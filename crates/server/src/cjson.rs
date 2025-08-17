use palpo_core::serde::CanonicalJsonError;
use salvo::http::header::{CONTENT_TYPE, HeaderValue};
use salvo::http::{Response, StatusError};
use salvo::oapi::{
    self, Components, Content, EndpointOutRegister, Operation, RefOr, ToResponse, ToSchema,
};
use salvo::{Scribe, async_trait};
use serde::Serialize;

use crate::core::serde::CanonicalJsonValue;

pub struct Cjson<T>(pub T);

#[async_trait]
impl<T> Scribe for Cjson<T>
where
    T: Serialize + Send,
{
    fn render(self, res: &mut Response) {
        match try_to_bytes(&self.0) {
            Ok(bytes) => {
                res.headers_mut().insert(
                    CONTENT_TYPE,
                    HeaderValue::from_static("application/json; charset=utf-8"),
                );
                res.write_body(bytes).ok();
            }
            Err(e) => {
                tracing::error!(error = ?e, "JsonContent write error");
                res.render(StatusError::internal_server_error());
            }
        }
    }
}
impl<C> ToResponse for Cjson<C>
where
    C: ToSchema,
{
    fn to_response(components: &mut Components) -> RefOr<oapi::Response> {
        let schema = <C as ToSchema>::to_schema(components);
        oapi::Response::new("Response with json format data")
            .add_content("application/json", Content::new(schema))
            .into()
    }
}

impl<C> EndpointOutRegister for Cjson<C>
where
    C: ToSchema,
{
    #[inline]
    fn register(components: &mut Components, operation: &mut Operation) {
        operation
            .responses
            .insert("200", Self::to_response(components));
    }
}

fn try_to_bytes<T>(data: &T) -> Result<Vec<u8>, CanonicalJsonError>
where
    T: Serialize + Send,
{
    let value: CanonicalJsonValue = serde_json::to_value(data)?.try_into()?;
    Ok(serde_json::to_vec(&value)?)
}
