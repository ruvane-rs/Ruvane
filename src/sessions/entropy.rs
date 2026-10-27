use super::{SESSION_ID_BYTES, SessionError};
use std::{fs::File, io::Read};
#[cfg_attr(
    not(any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )),
    allow(dead_code)
)]
pub(super) enum Entropy {
    System(File),
    #[cfg(test)]
    Scripted(std::collections::VecDeque<Result<[u8; SESSION_ID_BYTES], SessionError>>),
}
impl Entropy {
    #[cfg(any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    pub(super) fn open() -> Result<Self, SessionError> {
        use std::os::unix::fs::FileTypeExt;
        #[cfg(target_os = "linux")]
        let file = {
            use std::os::unix::fs::OpenOptionsExt;
            // Linux O_NONBLOCK; the kernel may still wait for initialization.
            std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(0x800)
                .open("/dev/random")
        };
        #[cfg(target_os = "macos")]
        let file = File::open("/dev/urandom");
        let file = file.map_err(|_| SessionError::EntropyUnavailable)?;
        if !file
            .metadata()
            .map_err(|_| SessionError::EntropyUnavailable)?
            .file_type()
            .is_char_device()
        {
            return Err(SessionError::EntropyUnavailable);
        }
        let mut source = Self::System(file);
        if source.read()? == [0; SESSION_ID_BYTES] {
            return Err(SessionError::EntropyUnavailable);
        }
        Ok(source)
    }
    #[cfg(not(any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )))]
    pub(super) fn open() -> Result<Self, SessionError> {
        Err(SessionError::UnsupportedPlatform)
    }
    pub(super) fn read(&mut self) -> Result<[u8; SESSION_ID_BYTES], SessionError> {
        match self {
            Self::System(file) => {
                let mut bytes = [0; SESSION_ID_BYTES];
                file.read_exact(&mut bytes)
                    .map_err(|_| SessionError::EntropyUnavailable)?;
                Ok(bytes)
            }
            #[cfg(test)]
            Self::Scripted(values) => values
                .pop_front()
                .unwrap_or(Err(SessionError::EntropyUnavailable)),
        }
    }
}
