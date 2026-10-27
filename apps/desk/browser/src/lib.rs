#![deny(unsafe_code)]
ruvane_browser::export_application!(
    ruvane_application::components::create_component,
    ruvane_application::validate_page,
    ruvane_application::REVISION
);
