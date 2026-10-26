//! Fixed action transport responses used by the browser harness.
use ruvane::{
    ActionData, ActionReply, ActionValue, Attribute, Document, InternalUrl, MediaType, Response,
    Router, StatusCode, Tag, element, module_script,
};

pub fn register(router: &mut Router) -> Result<(), Box<dyn std::error::Error>> {
    router.get("/action-tests", |_| {
        Response::interactive_html(
            Document::new(
                "Ruvane alpha.29 — Action tests",
                element(Tag::Main)
                    .child(element(Tag::H1).child("Ruvane / Action tests"))
                    .child(
                        element(Tag::P)
                            .attr(Attribute::Id, "status")
                            .child("Running…"),
                    )
                    .child(element(Tag::Pre).attr(Attribute::Id, "report"))
                    .child(module_script(
                        &InternalUrl::new("/assets/action-tests.js").unwrap(),
                    )),
            )
            .render(),
        )
    })?;
    router.asset(
        "/assets/action-tests.js",
        include_bytes!("../../web/action-tests.mjs"),
        MediaType::JavaScript,
    )?;
    router.post_form("/actions/fixtures/{mode}", |request| {
        let Ok(mode) = request.param_as::<u32>("mode") else {
            return Response::text("invalid").with_status(StatusCode::BadRequest);
        };
        let mut data = ActionData::default();
        data.insert("message", ActionValue::Text("ok".into()))
            .unwrap();
        match mode {
            1 => {
                std::thread::sleep(std::time::Duration::from_millis(250));
                Response::action(&ActionReply::success(data))
            }
            2 => Response::bytes(
                b"RVA1\0\x01\0\x01a\x01\x01\0\0\0\xff".to_vec(),
                MediaType::Action,
            ),
            3 => Response::bytes(vec![0; 65537], MediaType::Action),
            4 => Response::text("wrong MIME"),
            5 => Response::see_other(InternalUrl::new("/tasks").unwrap()),
            6 => Response::text("failure").with_status(StatusCode::InternalServerError),
            7 => Response::action(&ActionReply::validation(data)),
            8 => Response::bytes(b"RVA1\0\x01\0\x01a\x03\x02".to_vec(), MediaType::Action),
            9 => Response::action(&ActionReply::success(data)).with_status(StatusCode::BadRequest),
            _ => Response::action(&ActionReply::success(data)),
        }
    })?;
    Ok(())
}
