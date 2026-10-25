use crate::http;
use crate::{Method, Response, Router, StatusCode};
use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const IO_SLICE: Duration = Duration::from_millis(50);

/// Resource budgets for the loopback-only development server.
///
/// All fields are validated when binding. These limits bound framework I/O,
/// not memory or CPU consumed by application handlers or the operating system.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Worker threads, from 1 to 64; defaults to 4.
    pub workers: usize,
    /// Queued connections, from 1 to 1024; defaults to 32.
    pub pending_connections: usize,
    /// Header block bytes including CRLF delimiters, from 32 to 1 MiB.
    /// Defaults to 16 KiB.
    pub max_header_bytes: usize,
    /// Request line bytes excluding CRLF, from 14 to 64 KiB.
    /// Must leave room for delimiters within `max_header_bytes`; defaults to 4096.
    pub max_request_line_bytes: usize,
    /// Header fields, from 1 to 1024; defaults to 64.
    pub max_headers: usize,
    /// Application response body bytes, from 64 to 64 MiB; defaults to 1 MiB.
    /// An oversized result is replaced by a small 500 response after it is built.
    pub max_response_body_bytes: usize,
    /// Total time from accepting a connection to receiving its complete headers,
    /// including time in the queue. From 1 ms to 60 seconds; defaults to 5 seconds.
    pub request_timeout: Duration,
    /// Total time to write a response, from 1 ms to 60 seconds; defaults to 5 seconds.
    pub write_timeout: Duration,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            workers: 4,
            pending_connections: 32,
            max_header_bytes: 16 * 1024,
            max_request_line_bytes: 4096,
            max_headers: 64,
            max_response_body_bytes: 1024 * 1024,
            request_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
        }
    }
}

impl ServerConfig {
    fn validate(&self) -> io::Result<()> {
        let valid = (1..=64).contains(&self.workers)
            && (1..=1024).contains(&self.pending_connections)
            && (32..=1024 * 1024).contains(&self.max_header_bytes)
            && (14..=64 * 1024).contains(&self.max_request_line_bytes)
            && self.max_request_line_bytes <= self.max_header_bytes - 4
            && (1..=1024).contains(&self.max_headers)
            && (64..=64 * 1024 * 1024).contains(&self.max_response_body_bytes)
            && (Duration::from_millis(1)..=Duration::from_secs(60)).contains(&self.request_timeout)
            && (Duration::from_millis(1)..=Duration::from_secs(60)).contains(&self.write_timeout);
        if valid {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "server limits outside supported ranges",
            ))
        }
    }
}

/// A cloneable request to stop accepting and close queued or active socket I/O.
///
/// Shutdown is permanent. [`Server::run`] joins workers, so trusted application
/// handlers already executing must finish themselves. No thread is forcibly killed.
#[derive(Clone, Debug)]
pub struct ShutdownHandle {
    stopped: Arc<AtomicBool>,
}

impl ShutdownHandle {
    /// Requests server shutdown. Calling this more than once is harmless.
    pub fn shutdown(&self) {
        self.stopped.store(true, Ordering::Release);
    }
}

/// A bounded HTTP development server that binds only to a loopback IP address.
///
/// This is a limited, bodyless HTTP/1.1 profile, not a general HTTP/1.1 or TLS
/// implementation. Each connection receives at most one response and is closed.
/// Host must name `localhost` or the bound IP with the actual port (port 80
/// may be omitted). Overload closes newly accepted connections without a response.
#[derive(Debug)]
pub struct Server {
    listener: TcpListener,
    router: Router,
    config: ServerConfig,
    shutdown: ShutdownHandle,
}

impl Server {
    /// Binds a loopback socket using the default resource budgets.
    ///
    /// Use port zero to let the operating system choose an available port.
    pub fn bind(address: SocketAddr, router: Router) -> io::Result<Self> {
        Self::bind_with_config(address, router, ServerConfig::default())
    }

    /// Validates custom budgets and binds a loopback socket.
    ///
    /// Wildcard and non-loopback addresses are rejected before opening a socket.
    pub fn bind_with_config(
        address: SocketAddr,
        router: Router,
        config: ServerConfig,
    ) -> io::Result<Self> {
        config.validate()?;
        if !address.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "only loopback addresses are supported",
            ));
        }
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            router,
            config,
            shutdown: ShutdownHandle {
                stopped: Arc::new(AtomicBool::new(false)),
            },
        })
    }

    /// Returns the bound address, including an operating-system-assigned port.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Returns a handle that can stop this server from another thread.
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        self.shutdown.clone()
    }

    /// Runs the accept loop on the calling thread and joins all workers on exit.
    ///
    /// Accepting never waits for space in the bounded queue. Socket operations
    /// periodically check shutdown and absolute deadlines, even if bytes keep
    /// arriving. Handler panics are caught when compiled with unwinding; a
    /// `panic = "abort"` application instead aborts its process as configured.
    pub fn run(self) -> io::Result<()> {
        let address = self.local_addr()?;
        let router = Arc::new(self.router);
        let config = Arc::new(self.config);
        let stopped = self.shutdown.stopped;
        let (sender, receiver) = mpsc::sync_channel::<Job>(config.pending_connections);
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(config.workers);
        let mut outcome = Ok(());

        for index in 0..config.workers {
            let receiver = Arc::clone(&receiver);
            let router = Arc::clone(&router);
            let config = Arc::clone(&config);
            let stopped = Arc::clone(&stopped);
            match thread::Builder::new()
                .name(format!("ruvane-{index}"))
                .spawn(move || {
                    loop {
                        let job = receiver
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .recv();
                        let Ok(job) = job else { break };
                        if stopped.load(Ordering::Acquire) {
                            break;
                        }
                        serve(job, &router, &config, address, &stopped);
                    }
                }) {
                Ok(worker) => workers.push(worker),
                Err(error) => {
                    outcome = Err(error);
                    break;
                }
            }
        }
        // Do not keep the channel alive if every worker exits. A weak reference
        // still lets shutdown discard queued sockets before joining handlers.
        let pending = Arc::downgrade(&receiver);
        drop(receiver);
        if outcome.is_ok() {
            while !stopped.load(Ordering::Acquire) {
                match self.listener.accept() {
                    Ok((stream, _)) => {
                        let job = Job {
                            stream,
                            accepted_at: Instant::now(),
                        };
                        match sender.try_send(job) {
                            Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                            Err(mpsc::TrySendError::Disconnected(_)) => {
                                outcome = Err(io::Error::other("server workers unavailable"));
                                break;
                            }
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(POLL_INTERVAL);
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => {
                        outcome = Err(error);
                        break;
                    }
                }
            }
        }
        stopped.store(true, Ordering::Release);
        drop(self.listener);
        drop(sender);
        if let Some(receiver) = pending.upgrade() {
            let receiver = receiver.lock().unwrap_or_else(|error| error.into_inner());
            while receiver.try_recv().is_ok() {}
        }
        for worker in workers {
            if worker.join().is_err() && outcome.is_ok() {
                outcome = Err(io::Error::other("server worker failed"));
            }
        }
        outcome
    }
}

struct Job {
    stream: TcpStream,
    accepted_at: Instant,
}

fn serve(
    mut job: Job,
    router: &Router,
    config: &ServerConfig,
    address: SocketAddr,
    stopped: &AtomicBool,
) {
    if job.stream.set_nonblocking(false).is_err() {
        return;
    }
    let mut bytes = Vec::new();
    let received = read_headers(
        &mut job.stream,
        &mut bytes,
        config,
        job.accepted_at,
        stopped,
    );
    // Suppress error bodies too when a HEAD request line was recognizable.
    let mut head = bytes.starts_with(b"HEAD ");
    let response = match received.and_then(|end| {
        http::parse(
            &bytes[..end],
            config.max_request_line_bytes,
            config.max_headers,
        )
    }) {
        Ok(request) if valid_host(request.header("host").unwrap_or(""), address) => {
            head = request.method() == Method::Head;
            match catch_unwind(AssertUnwindSafe(|| router.dispatch(request))) {
                Ok(response) if response.body().len() <= config.max_response_body_bytes => response,
                _ => {
                    let error = router.error_response(StatusCode::InternalServerError);
                    if error.body().len() <= config.max_response_body_bytes {
                        error
                    } else {
                        Response::error(StatusCode::InternalServerError)
                    }
                }
            }
        }
        Ok(_) => Response::error(StatusCode::BadRequest),
        Err(status) => Response::error(status),
    };
    let deadline = Instant::now() + config.write_timeout;
    let headers = response.wire_headers();
    if write_until(&mut job.stream, headers.as_bytes(), deadline, stopped).is_ok() && !head {
        let _ = write_until(&mut job.stream, response.body(), deadline, stopped);
    }
}

fn valid_host(authority: &str, address: SocketAddr) -> bool {
    let (host, port) = if authority.starts_with('[') {
        if let Ok(socket) = authority.parse::<SocketAddr>() {
            return socket == address;
        }
        if let Some(host) = authority
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
        {
            (host, 80)
        } else {
            return false;
        }
    } else if let Some((host, port)) = authority.split_once(':') {
        if port.is_empty() || !port.bytes().all(|byte| byte.is_ascii_digit()) {
            return false;
        }
        let Ok(port) = port.parse::<u16>() else {
            return false;
        };
        (host, port)
    } else {
        (authority, 80)
    };
    port == address.port()
        && (host.eq_ignore_ascii_case("localhost")
            || host.parse::<IpAddr>().ok() == Some(address.ip()))
}

fn read_headers(
    stream: &mut TcpStream,
    bytes: &mut Vec<u8>,
    config: &ServerConfig,
    accepted_at: Instant,
    stopped: &AtomicBool,
) -> Result<usize, StatusCode> {
    let deadline = accepted_at + config.request_timeout;
    let mut chunk = [0; 1024];
    let mut first_line_end = None;
    loop {
        if stopped.load(Ordering::Acquire) {
            return Err(StatusCode::RequestTimeout);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(StatusCode::RequestTimeout);
        }
        if bytes.len() == config.max_header_bytes {
            return Err(StatusCode::RequestHeaderFieldsTooLarge);
        }
        stream
            .set_read_timeout(Some(remaining.min(IO_SLICE)))
            .map_err(|_| StatusCode::BadRequest)?;
        let available = chunk.len().min(config.max_header_bytes - bytes.len());
        let count = match stream.read(&mut chunk[..available]) {
            Ok(0) => return Err(StatusCode::BadRequest),
            Ok(count) => count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(_) => return Err(StatusCode::BadRequest),
        };
        let old_length = bytes.len();
        bytes.extend_from_slice(&chunk[..count]);
        // Scan only newly received bytes plus the delimiter overlap. This
        // remains linear even when a client sends one byte per read.
        for end in old_length.saturating_sub(3)..bytes.len() {
            if first_line_end.is_none() && end >= 1 && &bytes[end - 1..=end] == b"\r\n" {
                first_line_end = Some(end - 1);
                if end - 1 > config.max_request_line_bytes {
                    return Err(StatusCode::UriTooLong);
                }
            }
            if end >= 3 && &bytes[end - 3..=end] == b"\r\n\r\n" {
                if Instant::now() >= deadline {
                    return Err(StatusCode::RequestTimeout);
                }
                return Ok(end + 1);
            }
        }
        if first_line_end.is_none() && bytes.len() > config.max_request_line_bytes + 1 {
            return Err(StatusCode::UriTooLong);
        }
    }
}

fn write_until(
    stream: &mut TcpStream,
    mut bytes: &[u8],
    deadline: Instant,
    stopped: &AtomicBool,
) -> io::Result<()> {
    while !bytes.is_empty() {
        if stopped.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "server stopped"));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "response deadline exceeded",
            ));
        }
        stream.set_write_timeout(Some(remaining.min(IO_SLICE)))?;
        match stream.write(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "connection closed",
                ));
            }
            Ok(count) => bytes = &bytes[count..],
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_policy_checks_address_port_and_ipv6() {
        let address = "127.0.0.1:3000".parse().unwrap();
        for host in ["localhost:3000", "LOCALHOST:3000", "127.0.0.1:3000"] {
            assert!(valid_host(host, address), "{host}");
        }
        for host in [
            "evil.test:3000",
            "localhost.evil:3000",
            "localhost",
            "localhost:80",
            "localhost:+3000",
            "user@localhost:3000",
            "localhost:3000,evil",
            "[::1]:3000",
            "127.0.0.2:3000",
            "localhost:99999",
            "localhost:",
        ] {
            assert!(!valid_host(host, address), "{host}");
        }
        assert!(valid_host("localhost", "127.0.0.1:80".parse().unwrap()));
        assert!(valid_host("[::1]:3000", "[::1]:3000".parse().unwrap()));
        assert!(valid_host(
            "[0:0:0:0:0:0:0:1]:3000",
            "[::1]:3000".parse().unwrap()
        ));
        assert!(valid_host("[::1]", "[::1]:80".parse().unwrap()));
    }
}
