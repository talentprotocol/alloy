use crate::{Response, ResponsePayload, RpcError, RpcReturn};
use serde_json::value::RawValue;
use std::borrow::Borrow;

/// The result of a JSON-RPC request.
///
/// Either a success response, an error response, or a non-response error. The
/// non-response error is intended to be used for errors returned by a
/// transport, or serde errors.
///
/// The common cases are:
/// - `Ok(T)` - The server returned a successful response.
/// - `Err(RpcError::ErrorResponse(ErrResp))` - The server returned an error response.
/// - `Err(RpcError::SerError(E))` - A serialization error occurred.
/// - `Err(RpcError::DeserError { err: E, text: String })` - A deserialization error occurred.
/// - `Err(RpcError::TransportError(E))` - Some client-side or communication error occurred.
pub type RpcResult<T, E, ErrResp = Box<RawValue>> = Result<T, RpcError<E, ErrResp>>;

/// A partially deserialized [`RpcResult`], borrowing from the deserializer.
pub type BorrowedRpcResult<'a, E> = RpcResult<&'a RawValue, E, &'a RawValue>;

/// Transform a transport response into an [`RpcResult`], discarding the [`Id`].
///
/// [`Id`]: crate::Id
pub fn transform_response<T, E, ErrResp>(
    response: Response<T, ErrResp>,
) -> Result<T, RpcError<E, ErrResp>>
where
    ErrResp: RpcReturn,
{
    match response {
        Response { payload: ResponsePayload::Failure(err_resp), .. } => {
            Err(RpcError::err_resp(err_resp))
        }
        Response { payload: ResponsePayload::Success(result), .. } => Ok(result),
    }
}

/// Transform a transport outcome into an [`RpcResult`], discarding the [`Id`].
///
/// [`Id`]: crate::Id
pub fn transform_result<T, E, ErrResp>(
    response: Result<Response<T, ErrResp>, E>,
) -> Result<T, RpcError<E, ErrResp>>
where
    ErrResp: RpcReturn,
{
    match response {
        Ok(resp) => transform_response(resp),
        Err(e) => Err(RpcError::Transport(e)),
    }
}

/// Attempt to deserialize the `Ok(_)` variant of an [`RpcResult`].
pub fn try_deserialize_ok<J, T, E, ErrResp>(
    result: RpcResult<J, E, ErrResp>,
) -> RpcResult<T, E, ErrResp>
where
    J: Borrow<RawValue>,
    T: RpcReturn,
    ErrResp: RpcReturn,
{
    let json = result?;
    let json = json.borrow().get();

    // Parse into Value so we can manipulate it
    let mut value: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            println!("Failed to parse JSON into Value: {}", e);
            return Err(RpcError::deser_err(e, json));
        }
    };

    if let serde_json::Value::Object(ref mut map) = value {
        if let Some(serde_json::Value::Array(ref mut txs)) = map.get_mut("transactions") {
            txs.retain(|tx| match tx.get("type") {
                Some(serde_json::Value::String(t)) => t != "0x7e",
                _ => true,
            });
        }
    }

    // Serialize back to string for final deserialization
    let cleaned_json = match serde_json::to_string(&value) {
        Ok(s) => s,
        Err(e) => {
            println!("Failed to serialize cleaned JSON: {}", e);
            return Err(RpcError::deser_err(e, json));
        }
    };

    trace!(ty=%std::any::type_name::<T>(), %cleaned_json, "deserializing response");
    if let Err(e) = std::fs::write("error-temp-debug.json", &cleaned_json) {
        println!("Failed to write error JSON to file: {}", e);
    }

    serde_json::from_str(&cleaned_json)
        .inspect(|response| trace!(?response, "deserialized response"))
        .inspect_err(|err| trace!(?err, "failed to deserialize response"))
        .map_err(|err| RpcError::deser_err(err, cleaned_json))
}
