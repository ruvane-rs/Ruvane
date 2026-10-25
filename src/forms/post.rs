use super::{FormError, valid_label, valid_name, valid_text};
use crate::html::escape_into;
use crate::{Html, InternalUrl, MAX_COMPONENT_BYTES, MAX_QUERY_PAIRS, ParamError};
use std::str::FromStr;

/// Maximum encoded bytes in a POST form body.
pub const MAX_POST_BODY_BYTES: usize = 64 * 1024;

fn decode(raw: &str, multiline: bool) -> Result<String, FormError> {
    let limit = MAX_COMPONENT_BYTES * if multiline { 2 } else { 1 };
    if raw.len() > limit * 3 {
        return Err(FormError::TooLarge);
    }
    let hex = |byte: u8| (byte as char).to_digit(16).map(|n| n as u8);
    let mut bytes = Vec::with_capacity(raw.len().min(limit));
    let mut input = raw.bytes();
    while let Some(byte) = input.next() {
        if bytes.len() == limit {
            return Err(FormError::TooLarge);
        }
        bytes.push(match byte {
            b'%' => {
                let high = input
                    .next()
                    .and_then(hex)
                    .ok_or(FormError::InvalidEncoding)?;
                let low = input
                    .next()
                    .and_then(hex)
                    .ok_or(FormError::InvalidEncoding)?;
                high * 16 + low
            }
            b'+' => b' ',
            33..=126 => byte,
            _ => return Err(FormError::InvalidEncoding),
        });
    }
    let decoded = String::from_utf8(bytes).map_err(|_| FormError::InvalidUtf8)?;
    let value = if multiline {
        decoded.replace("\r\n", "\n")
    } else {
        decoded
    };
    if value.len() > MAX_COMPONENT_BYTES {
        return Err(FormError::TooLarge);
    }
    if value
        .chars()
        .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\t')))
    {
        return Err(FormError::InvalidText);
    }
    Ok(value)
}

/// Ordered POST fields with unique ASCII names and bounded UTF-8 values.
///
/// CRLF becomes LF in values. Tabs and LF are permitted; other controls are
/// rejected. Query parameters are parsed separately and are never merged here.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FormData {
    pairs: Vec<(String, String)>,
}

impl FormData {
    /// Parses this framework's URL-encoded form profile without a leading `?`.
    ///
    /// ```
    /// let form = ruvane::FormData::parse(b"name=Rust+%2B+HTML&notes=one%0D%0Atwo&done=on").unwrap();
    /// assert_eq!(form.get("notes"), Some("one\ntwo"));
    /// assert_eq!(form.checkbox("done"), Ok(true));
    /// assert_eq!(form.checkbox("missing"), Ok(false));
    /// ```
    pub fn parse(bytes: &[u8]) -> Result<Self, FormError> {
        if bytes.len() > MAX_POST_BODY_BYTES {
            return Err(FormError::TooLarge);
        }
        if !bytes.is_ascii() {
            return Err(FormError::InvalidEncoding);
        }
        let raw = std::str::from_utf8(bytes).map_err(|_| FormError::InvalidEncoding)?;
        let mut form = Self::default();
        if raw.is_empty() {
            return Ok(form);
        }
        for pair in raw.split('&') {
            if form.pairs.len() == MAX_QUERY_PAIRS {
                return Err(FormError::TooManyFields);
            }
            let (key, value) = pair.split_once('=').ok_or(FormError::InvalidEncoding)?;
            let key = decode(key, false)?;
            valid_name(&key)?;
            if form.pairs.iter().any(|(name, _)| name == &key) {
                return Err(FormError::DuplicateName);
            }
            form.pairs.push((key, decode(value, true)?));
        }
        Ok(form)
    }

    /// Looks up a decoded value by its exact field name.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// Converts a field using the selected Rust type's syntax.
    pub fn get_as<T: FromStr>(&self, name: &str) -> Result<T, ParamError> {
        crate::url::parse_value(self.get(name))
    }

    /// Reads a native checkbox: missing is false, `on` is true.
    pub fn checkbox(&self, name: &str) -> Result<bool, ParamError> {
        match self.get(name) {
            None => Ok(false),
            Some("on") => Ok(true),
            _ => Err(ParamError::Invalid),
        }
    }

    /// Whether every submitted name is in an application's declared field list.
    pub fn has_only(&self, names: &[&str]) -> bool {
        self.pairs
            .iter()
            .all(|(key, _)| names.contains(&key.as_str()))
    }

    /// Decoded fields in their original order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Number of submitted fields.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Whether no fields were submitted.
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

#[derive(Debug)]
enum Control {
    Text {
        label: String,
        value: String,
        multiline: bool,
    },
    Checkbox {
        label: String,
        checked: bool,
    },
    Hidden(String),
}
#[derive(Debug)]
struct Field {
    name: String,
    control: Control,
    invalid: bool,
}

/// A native POST form with a fixed internal action and labeled controls.
///
/// The receiving route must use `Router::post_form`. No script, inline handler,
/// custom enctype or control that overrides method/action is emitted.
#[must_use]
#[derive(Debug)]
pub struct PostForm {
    action: InternalUrl,
    fields: Vec<Field>,
    submit: String,
}

impl PostForm {
    /// Starts an empty form. Put parameters in fields, not the action query.
    pub fn new(action: InternalUrl) -> Result<Self, FormError> {
        if action.as_str().contains('?') {
            return Err(FormError::ActionHasQuery);
        }
        Ok(Self {
            action,
            fields: Vec::new(),
            submit: "Submit".into(),
        })
    }

    fn add(mut self, name: String, control: Control) -> Result<Self, FormError> {
        valid_name(&name)?;
        if self.fields.iter().any(|field| field.name == name) {
            return Err(FormError::DuplicateName);
        }
        if self.fields.len() == MAX_QUERY_PAIRS {
            return Err(FormError::TooManyFields);
        }
        self.fields.push(Field {
            name,
            control,
            invalid: false,
        });
        Ok(self)
    }

    /// Adds a labeled single-line text field.
    pub fn text(
        self,
        name: impl Into<String>,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, FormError> {
        let (label, value) = (label.into(), value.into());
        valid_label(&label)?;
        valid_text(&value)?;
        self.add(
            name.into(),
            Control::Text {
                label,
                value,
                multiline: false,
            },
        )
    }

    /// Adds a labeled multiline field. Values use LF line endings.
    pub fn textarea(
        self,
        name: impl Into<String>,
        label: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, FormError> {
        let (label, value) = (label.into(), value.into());
        valid_label(&label)?;
        if value.len() > MAX_COMPONENT_BYTES {
            return Err(FormError::TooLarge);
        }
        if value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        {
            return Err(FormError::InvalidText);
        }
        self.add(
            name.into(),
            Control::Text {
                label,
                value,
                multiline: true,
            },
        )
    }

    /// Adds a checkbox whose submitted value is `on` when checked.
    pub fn checkbox(
        self,
        name: impl Into<String>,
        label: impl Into<String>,
        checked: bool,
    ) -> Result<Self, FormError> {
        let label = label.into();
        valid_label(&label)?;
        self.add(name.into(), Control::Checkbox { label, checked })
    }

    /// Adds a hidden field. Its value remains untrusted input on submission.
    pub fn hidden(
        self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Self, FormError> {
        let value = value.into();
        valid_text(&value)?;
        self.add(name.into(), Control::Hidden(value))
    }

    /// Marks a declared visible field as invalid.
    pub fn invalid(mut self, name: &str) -> Result<Self, FormError> {
        let field = self
            .fields
            .iter_mut()
            .find(|field| field.name == name && !matches!(field.control, Control::Hidden(_)))
            .ok_or(FormError::InvalidName)?;
        field.invalid = true;
        Ok(self)
    }

    /// Sets the unnamed submit button's label.
    pub fn submit_label(mut self, label: impl Into<String>) -> Result<Self, FormError> {
        let label = label.into();
        valid_label(&label)?;
        self.submit = label;
        Ok(self)
    }

    /// Renders escaped controls that work without JavaScript.
    pub fn render(self) -> Html {
        let mut html = String::from("<form method=\"post\" action=\"");
        escape_into(&mut html, self.action.as_str());
        html.push_str(
            "\" enctype=\"application/x-www-form-urlencoded\" accept-charset=\"UTF-8\" novalidate>",
        );
        for field in self.fields {
            let (label, tag) = match &field.control {
                Control::Text {
                    label, multiline, ..
                } => (
                    Some(label),
                    if *multiline {
                        "<textarea"
                    } else {
                        "<input type=\"text\""
                    },
                ),
                Control::Checkbox { label, .. } => (Some(label), "<input type=\"checkbox\""),
                Control::Hidden(_) => (None, "<input type=\"hidden\""),
            };
            let visible = label.is_some();
            if let Some(label) = label {
                html.push_str("<label><span>");
                escape_into(&mut html, label);
                html.push_str("</span>");
            }
            html.push_str(tag);
            html.push_str(" name=\"");
            escape_into(&mut html, &field.name);
            html.push('"');
            if visible {
                html.push_str(if field.invalid {
                    " aria-invalid=\"true\""
                } else {
                    " aria-invalid=\"false\""
                });
            }
            match field.control {
                Control::Text {
                    value, multiline, ..
                } => {
                    html.push_str(" maxlength=\"1024\"");
                    if multiline {
                        html.push('>');
                        if value.starts_with('\n') {
                            html.push('\n');
                        }
                        escape_into(&mut html, &value);
                        html.push_str("</textarea>");
                    } else {
                        html.push_str(" value=\"");
                        escape_into(&mut html, &value);
                        html.push_str("\">");
                    }
                }
                Control::Checkbox { checked, .. } => {
                    html.push_str(" value=\"on\"");
                    if checked {
                        html.push_str(" checked");
                    }
                    html.push('>');
                }
                Control::Hidden(value) => {
                    html.push_str(" value=\"");
                    escape_into(&mut html, &value);
                    html.push_str("\">");
                }
            }
            if visible {
                html.push_str("</label>");
            }
        }
        html.push_str("<button type=\"submit\">");
        escape_into(&mut html, &self.submit);
        html.push_str("</button></form>");
        Html::rendered(html)
    }
}

impl From<PostForm> for Html {
    fn from(form: PostForm) -> Self {
        form.render()
    }
}
