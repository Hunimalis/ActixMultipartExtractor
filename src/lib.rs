use actix_web::{dev::Payload, Error, FromRequest, HttpRequest};
use futures_util::{future::LocalBoxFuture, stream::StreamExt as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::ops::{Deref, DerefMut};

pub struct Multipart<T>(T);

impl<T> Multipart<T> {
    pub fn new(data: T) -> Self {
        Multipart::<T>(data)
    }
}

impl<T> Deref for Multipart<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> DerefMut for Multipart<T> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: serde::de::DeserializeOwned> FromRequest for Multipart<T> {
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Multipart<T>, Self::Error>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let multipart = actix_multipart::Multipart::new(req.headers(), payload.take());

        Box::pin(async move {
            match parse::<T>(multipart).await {
                Ok(res) => Ok(Multipart::<T>(res)),
                Err(_) => Err(actix_web::error::ErrorBadRequest("")),
            }
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct File {
    r#type: String,
    name: String,
    data: Vec<u8>,
}

impl File {
    pub fn r#type(&self) -> &String {
        &self.r#type
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn data(&self) -> &Vec<u8> {
        &self.data
    }
}

fn merge(obj: &mut serde_json::Value, field: (String, Value)) {
    if let Value::Object(ref mut map) = obj {
        if &field.0[field.0.len() - 2..] == "[]" {
            let name = &field.0[..field.0.len() - 2];

            // if array field doesn't exist at time, we create it
            if map.get(name).is_none() {
                map.insert(name.to_string(), json!(Vec::<File>::new()));
            }

            let array = map.get_mut(name).unwrap().as_array_mut().unwrap();
            array.push(field.1);
        } else {
            map.insert(field.0, field.1);
        }
    }
}

async fn parse<T: serde::de::DeserializeOwned>(
    mut payload: actix_multipart::Multipart,
) -> Result<T, ()> {
    let mut obj = serde_json::json!({});

    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(item) => item,
            Err(_) => return Err(())
        };

        let _type = field.content_type().to_string();
        let name = field.content_disposition().get_name().unwrap().to_string();

        // TODO : check if is_attachment() should be prefered
        match &field.content_disposition().get_filename() {
            // Is an attachment
            Some(filename) => {
                let mut d = vec![];
                let _type = _type.to_string();
                let filename = filename.to_string();

                while let Some(chunk) = field.next().await {
                    match chunk {
                        Ok(data) => {
                            d.append(&mut data.to_vec()); // = data.to_vec();
                        }
                        Err(e) => return Err(())
                    }
                }

                merge(
                    &mut obj,
                    (
                        name.clone(),
                        json!(File {
                            data: d,
                            name: filename,
                            r#type: _type
                        }),
                    ),
                );
            }
            // Is a simple field
            None => {
                if let Some(value) = field.next().await {
                    match value {
                        Ok(value) => match std::str::from_utf8(&value) {
                            Ok(value) => match value.parse::<isize>() {
                                Ok(value) => merge(
                                    &mut obj,
                                    (
                                        name.to_string(),
                                        Value::Number(serde_json::Number::from(value)),
                                    ),
                                ),
                                Err(_) => match value {
                                    "true" | "false" => merge(
                                        &mut obj,
                                        (name.to_string(), Value::Bool(value == "true")),
                                    ),
                                    _ => merge(
                                        &mut obj,
                                        (name.to_string(), Value::String(value.to_owned())),
                                    ),
                                },
                            },
                            Err(_) => merge(&mut obj, (name.to_string(), Value::Null)),
                        },
                        Err(_) => merge(&mut obj, (name.to_string(), Value::Null)),
                    }
                }
            }
        }
    }

    match serde_json::from_value::<T>(obj) {
        Ok(obj) => Ok(obj),
        Err(e) => Err(())
    }
}
