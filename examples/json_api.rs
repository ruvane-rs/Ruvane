#[path = "support/json_tasks.rs"]
mod json_tasks;
use ruvane::*;
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = std::env::args()
        .nth(1)
        .map(|value| value.parse::<u16>())
        .transpose()?
        .unwrap_or(8080);
    let store = SessionStore::new(
        SessionLimits::default(),
        SessionCookie::local_http("ruvane_json_demo")?,
    )?;
    let quota = RateLimiter::new(RateLimitPolicy::new(20, 60)?, RateLimitLimits::default());
    let public = RateLimiter::new(RateLimitPolicy::new(5, 60)?, RateLimitLimits::default());
    let tasks = Arc::new(json_tasks::Tasks::new()?);
    let mut app = App::new();
    app.asset(
        "/json-demo.mjs",
        include_bytes!("support/json_api.mjs"),
        MediaType::JavaScript,
    )?;
    let csrf = store.clone();
    let open = store.clone();
    let logout = store.clone();
    let mut routes = FileRouteRegistry::new(&mut app);
    routes.endpoint(
        "root/route.rs",
        "/",
        FileEndpoint::new().get(|_| {
            Response::interactive_html(
                Document::new(
                    "Ruvane JSON API",
                    fragment([
                        element(Tag::H1).child("Ruvane JSON API").render(),
                        element(Tag::P)
                            .child("Local demo sessions. No real user authentication.")
                            .render(),
                        module_script(&InternalUrl::new("/json-demo.mjs").unwrap()),
                    ]),
                )
                .render(),
            )
        }),
    )?;
    routes.endpoint(
        "demo-session/route.rs",
        "/demo-session",
        FileEndpoint::new()
            .post_form(move |request| {
                let active = match open.lookup(request) {
                    Ok(value) => value.is_some(),
                    Err(_) => return json_tasks::error(StatusCode::ServiceUnavailable),
                };
                let response = Response::json(&JsonValue::from(true));
                if active {
                    return response;
                }
                match open.create(SessionData::default()) {
                    Ok(cookie) => response.with_cookie(cookie).unwrap(),
                    Err(_) => json_tasks::error(StatusCode::ServiceUnavailable),
                }
            })
            .guard(public.guard()),
    )?;
    routes.endpoint(
        "csrf-token/route.rs",
        "/csrf-token",
        FileEndpoint::new().get(move |request| csrf.csrf_response(request)),
    )?;
    routes.endpoint(
        "api/tasks/route.rs",
        "/api/tasks",
        json_tasks::endpoint(tasks.clone())
            .guard(store.csrf_guard())
            .guard(quota.session_guard(store.clone())),
    )?;
    routes.endpoint(
        "api/tasks/[id]/route.rs",
        "/api/tasks/{id}",
        json_tasks::resource_endpoint(tasks)
            .guard(store.csrf_guard())
            .guard(quota.session_guard(store.clone())),
    )?;
    routes.endpoint(
        "api/logout/route.rs",
        "/api/logout",
        FileEndpoint::new()
            .post_json(move |request| {
                if !request
                    .json()
                    .and_then(JsonValue::as_object)
                    .is_some_and(|members| members.is_empty())
                {
                    return json_tasks::error(StatusCode::BadRequest);
                }
                let session = match logout.lookup(request) {
                    Ok(Some(session)) => session,
                    _ => return json_tasks::error(StatusCode::Forbidden),
                };
                if logout.revoke(&session).is_err() {
                    return json_tasks::error(StatusCode::ServiceUnavailable);
                }
                Response::json(&JsonValue::from(false))
                    .with_cookie(logout.removal_cookie())
                    .unwrap()
            })
            .guard(store.csrf_guard())
            .guard(quota.session_guard(store)),
    )?;
    routes.finish()?;
    let server = Server::bind(format!("127.0.0.1:{port}").parse()?, app.router(None)?)?;
    println!("Ruvane JSON demo at http://{}", server.local_addr()?);
    server.run()?;
    Ok(())
}
