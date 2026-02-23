use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl Response {
    pub fn success(id: Value, result: Value) -> Self {
        Response {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, error: RpcError) -> Self {
        Response {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

impl RpcError {
    pub fn parse_error() -> Self {
        RpcError {
            code: -32700,
            message: "Parse error".to_string(),
            data: None,
        }
    }

    pub fn invalid_request() -> Self {
        RpcError {
            code: -32600,
            message: "Invalid Request".to_string(),
            data: None,
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        RpcError {
            code: -32601,
            message: format!("Method not found: {}", method),
            data: None,
        }
    }

    pub fn invalid_params(msg: &str) -> Self {
        RpcError {
            code: -32602,
            message: format!("Invalid params: {}", msg),
            data: None,
        }
    }

    pub fn internal_error(msg: &str) -> Self {
        RpcError {
            code: -32603,
            message: "Internal error".to_string(),
            data: Some(Value::String(msg.to_string())),
        }
    }
}

/// Parse a JSON-RPC request line. Returns a vec of requests (batch support).
/// On parse failure, returns an error Response.
pub fn parse_request(line: &str) -> Result<Vec<Request>, Response> {
    let value: Value = serde_json::from_str(line).map_err(|_| {
        Response::error(Value::Null, RpcError::parse_error())
    })?;

    match value {
        Value::Array(arr) => {
            if arr.is_empty() {
                return Err(Response::error(Value::Null, RpcError::invalid_request()));
            }
            let mut requests = Vec::new();
            for item in arr {
                let req: Request = serde_json::from_value(item).map_err(|_| {
                    Response::error(Value::Null, RpcError::invalid_request())
                })?;
                requests.push(req);
            }
            Ok(requests)
        }
        Value::Object(_) => {
            let req: Request = serde_json::from_value(value).map_err(|_| {
                Response::error(Value::Null, RpcError::invalid_request())
            })?;
            Ok(vec![req])
        }
        _ => Err(Response::error(Value::Null, RpcError::invalid_request())),
    }
}

/// Validate that a request has jsonrpc: "2.0".
pub fn validate_request(req: &Request) -> Result<(), RpcError> {
    match &req.jsonrpc {
        Some(v) if v == "2.0" => Ok(()),
        _ => Err(RpcError::invalid_request()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_request() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"vault.info","params":{}}"#;
        let reqs = parse_request(line).unwrap();
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].method, "vault.info");
        assert_eq!(reqs[0].id, Some(Value::Number(1.into())));
    }

    #[test]
    fn test_parse_batch_request() {
        let line = r#"[{"jsonrpc":"2.0","id":1,"method":"a","params":{}},{"jsonrpc":"2.0","id":2,"method":"b","params":{}}]"#;
        let reqs = parse_request(line).unwrap();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].method, "a");
        assert_eq!(reqs[1].method, "b");
    }

    #[test]
    fn test_parse_malformed_json() {
        let line = "not json at all";
        let err = parse_request(line).unwrap_err();
        assert_eq!(err.error.as_ref().unwrap().code, -32700);
    }

    #[test]
    fn test_parse_empty_batch() {
        let line = "[]";
        let err = parse_request(line).unwrap_err();
        assert_eq!(err.error.as_ref().unwrap().code, -32600);
    }

    #[test]
    fn test_validate_request_ok() {
        let req = Request {
            jsonrpc: Some("2.0".to_string()),
            id: Some(Value::Number(1.into())),
            method: "test".to_string(),
            params: None,
        };
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn test_validate_request_bad_version() {
        let req = Request {
            jsonrpc: Some("1.0".to_string()),
            id: Some(Value::Number(1.into())),
            method: "test".to_string(),
            params: None,
        };
        assert!(validate_request(&req).is_err());
    }

    #[test]
    fn test_response_serialization() {
        let resp = Response::success(Value::Number(1.into()), serde_json::json!({"ok": true}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"result\""));
        assert!(!json.contains("\"error\""));
    }
}
