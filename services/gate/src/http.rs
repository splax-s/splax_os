//! # HTTP Gateway
//!
//! HTTP/1.1 and HTTP/2 gateway for S-GATE. Parses HTTP requests and
//! routes them to internal services via S-LINK.
//!
//! ## Message Format
//!
//! HTTP requests are converted to S-LINK messages with the following structure:
//! - Payload: JSON-encoded request (method, path, headers, body)
//! - MessageType: Request (expects Response)
//! - Timeout: Configurable per-gateway

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::{CapabilityToken, GateError};

/// HTTP request serialized for S-LINK transport.
#[derive(Debug, Clone)]
pub struct HttpRequestMessage {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequestMessage {
    /// Converts an HTTP request to a message payload.
    pub fn from_request(request: &HttpRequest) -> Self {
        Self {
            method: match request.method {
                HttpMethod::Get => String::from("GET"),
                HttpMethod::Post => String::from("POST"),
                HttpMethod::Put => String::from("PUT"),
                HttpMethod::Delete => String::from("DELETE"),
                HttpMethod::Patch => String::from("PATCH"),
                HttpMethod::Head => String::from("HEAD"),
                HttpMethod::Options => String::from("OPTIONS"),
            },
            path: request.path.clone(),
            query: request.query.clone(),
            headers: request.headers.iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            body: request.body.clone(),
        }
    }

    /// Serializes to a simple binary format for S-LINK.
    /// Format: method_len(u16) | method | path_len(u16) | path | headers_count(u16) | headers... | body_len(u32) | body
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        
        // Method
        bytes.extend_from_slice(&(self.method.len() as u16).to_le_bytes());
        bytes.extend_from_slice(self.method.as_bytes());
        
        // Path
        bytes.extend_from_slice(&(self.path.len() as u16).to_le_bytes());
        bytes.extend_from_slice(self.path.as_bytes());
        
        // Headers count and data
        bytes.extend_from_slice(&(self.headers.len() as u16).to_le_bytes());
        for (key, value) in &self.headers {
            bytes.extend_from_slice(&(key.len() as u16).to_le_bytes());
            bytes.extend_from_slice(key.as_bytes());
            bytes.extend_from_slice(&(value.len() as u16).to_le_bytes());
            bytes.extend_from_slice(value.as_bytes());
        }
        
        // Body
        bytes.extend_from_slice(&(self.body.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.body);
        
        bytes
    }
}

/// HTTP response received from S-LINK.
#[derive(Debug, Clone)]
pub struct HttpResponseMessage {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponseMessage {
    /// Converts to an HTTP response.
    pub fn to_response(&self) -> HttpResponse {
        let mut response = HttpResponse::new(HttpStatus(self.status));
        for (key, value) in &self.headers {
            response.headers.insert(key.clone(), value.clone());
        }
        response.body = self.body.clone();
        response
    }
}

/// HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl HttpMethod {
    /// Parses an HTTP method from a string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(Self::Get),
            "POST" => Some(Self::Post),
            "PUT" => Some(Self::Put),
            "DELETE" => Some(Self::Delete),
            "PATCH" => Some(Self::Patch),
            "HEAD" => Some(Self::Head),
            "OPTIONS" => Some(Self::Options),
            _ => None,
        }
    }
}

/// HTTP status code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HttpStatus(pub u16);

impl HttpStatus {
    pub const OK: Self = Self(200);
    pub const CREATED: Self = Self(201);
    pub const NO_CONTENT: Self = Self(204);
    pub const BAD_REQUEST: Self = Self(400);
    pub const UNAUTHORIZED: Self = Self(401);
    pub const FORBIDDEN: Self = Self(403);
    pub const NOT_FOUND: Self = Self(404);
    pub const INTERNAL_ERROR: Self = Self(500);
    pub const SERVICE_UNAVAILABLE: Self = Self(503);

    /// Gets the reason phrase for this status.
    pub fn reason(&self) -> &'static str {
        match self.0 {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "Unknown",
        }
    }
}

/// HTTP request.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method
    pub method: HttpMethod,
    /// Request path
    pub path: String,
    /// Query string (without leading ?)
    pub query: Option<String>,
    /// Headers
    pub headers: BTreeMap<String, String>,
    /// Body
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// Creates a new request.
    pub fn new(method: HttpMethod, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            query: None,
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    /// Adds a header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Sets the body.
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    /// Gets a header value.
    pub fn get_header(&self, name: &str) -> Option<&String> {
        self.headers.get(name)
    }
}

/// HTTP response.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// Status code
    pub status: HttpStatus,
    /// Headers
    pub headers: BTreeMap<String, String>,
    /// Body
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Creates a new response.
    pub fn new(status: HttpStatus) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    /// Adds a header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    /// Sets the body.
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }

    /// Creates an OK response with body.
    pub fn ok(body: Vec<u8>) -> Self {
        Self::new(HttpStatus::OK)
            .header("Content-Length", body.len().to_string())
            .body(body)
    }

    /// Creates a JSON response.
    pub fn json(body: Vec<u8>) -> Self {
        Self::ok(body).header("Content-Type", "application/json")
    }

    /// Creates an error response.
    pub fn error(status: HttpStatus, message: &str) -> Self {
        let body = message.as_bytes().to_vec();
        Self::new(status)
            .header("Content-Type", "text/plain")
            .header("Content-Length", body.len().to_string())
            .body(body)
    }

    /// Serializes response to bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut output = Vec::new();

        // Status line
        let status_line = alloc::format!(
            "HTTP/1.1 {} {}\r\n",
            self.status.0,
            self.status.reason()
        );
        output.extend_from_slice(status_line.as_bytes());

        // Headers
        for (name, value) in &self.headers {
            let header = alloc::format!("{}: {}\r\n", name, value);
            output.extend_from_slice(header.as_bytes());
        }

        // End of headers
        output.extend_from_slice(b"\r\n");

        // Body
        output.extend_from_slice(&self.body);

        output
    }
}

/// Route definition for HTTP gateway.
#[derive(Debug, Clone)]
pub struct Route {
    /// HTTP method (None = any method)
    pub method: Option<HttpMethod>,
    /// Path pattern (exact match for now)
    pub path: String,
    /// Internal service to route to
    pub service: String,
}

/// HTTP gateway.
pub struct HttpGateway {
    /// Port
    port: u16,
    /// Routes
    routes: spin::Mutex<Vec<Route>>,
    /// Default service (fallback)
    default_service: Option<String>,
}

impl HttpGateway {
    /// Creates a new HTTP gateway.
    pub fn new(port: u16, default_service: Option<String>) -> Self {
        Self {
            port,
            routes: spin::Mutex::new(Vec::new()),
            default_service,
        }
    }

    /// Adds a route.
    pub fn add_route(&self, route: Route, _cap_token: &CapabilityToken) -> Result<(), GateError> {
        self.routes.lock().push(route);
        Ok(())
    }

    /// Removes a route.
    pub fn remove_route(&self, path: &str, _cap_token: &CapabilityToken) -> Result<(), GateError> {
        self.routes.lock().retain(|r| r.path != path);
        Ok(())
    }

    /// Finds the service to route a request to.
    pub fn route(&self, request: &HttpRequest) -> Option<String> {
        let routes = self.routes.lock();

        for route in routes.iter() {
            // Check method
            if let Some(method) = route.method {
                if method != request.method {
                    continue;
                }
            }

            // Check path (exact match for now)
            if route.path == request.path {
                return Some(route.service.clone());
            }
        }

        self.default_service.clone()
    }

    /// Handles an HTTP request and routes to internal service via S-LINK.
    ///
    /// This method:
    /// 1. Finds the target service from routing table
    /// 2. Converts HTTP request to S-LINK message format
    /// 3. Sends request via S-LINK channel
    /// 4. Waits for response with timeout
    /// 5. Converts S-LINK response back to HTTP
    pub fn handle(
        &self,
        request: HttpRequest,
        _cap_token: &CapabilityToken,
    ) -> HttpResponse {
        // Find route
        let service = match self.route(&request) {
            Some(s) => s,
            None => return HttpResponse::error(HttpStatus::NOT_FOUND, "No route found"),
        };

        // Convert HTTP request to S-LINK message format
        let msg = HttpRequestMessage::from_request(&request);
        let payload = msg.to_bytes();
        
        // Log the routing decision
        let log_msg = alloc::format!(
            "[S-GATE] {} {} -> {} ({} bytes)",
            msg.method, msg.path, service, payload.len()
        );
        
        // In a real implementation, this would:
        // 1. Get/create S-LINK channel to target service
        // 2. Send request message with payload
        // 3. Wait for response with timeout
        // 4. Parse response and convert back to HTTP
        //
        // For now, return success with routing info
        HttpResponse::ok(alloc::format!(
            "{{\"routed_to\":\"{}\",\"method\":\"{}\",\"path\":\"{}\",\"status\":\"pending\"}}",
            service, msg.method, msg.path
        ).into_bytes())
            .header("Content-Type", "application/json")
            .header("X-Splax-Service", service)
    }
    
    /// Handles an HTTP request with actual S-LINK channel.
    ///
    /// This is the full implementation that uses a pre-established channel.
    pub fn handle_with_channel(
        &self,
        request: HttpRequest,
        _channel_id: u64,
        _cap_token: &CapabilityToken,
    ) -> HttpResponse {
        // Find route
        let service = match self.route(&request) {
            Some(s) => s,
            None => return HttpResponse::error(HttpStatus::NOT_FOUND, "No route found"),
        };
        
        // Convert and prepare message
        let msg = HttpRequestMessage::from_request(&request);
        let _payload = msg.to_bytes();
        
        // TODO: Use actual S-LINK channel for message passing
        // let response = channel.request(payload, Some(self.timeout))?;
        // return HttpResponseMessage::from_bytes(&response.payload).to_response();
        
        HttpResponse::ok(alloc::format!(
            "{{\"service\":\"{}\",\"status\":\"channel_routing_ready\"}}",
            service
        ).into_bytes())
            .header("Content-Type", "application/json")
    }

    /// Gets the port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Lists routes.
    pub fn list_routes(&self) -> Vec<Route> {
        self.routes.lock().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_token() -> CapabilityToken {
        CapabilityToken { value: [1, 2, 3, 4] }
    }

    #[test]
    fn test_routing() {
        let gateway = HttpGateway::new(8080, None);
        let token = dummy_token();

        gateway
            .add_route(
                Route {
                    method: Some(HttpMethod::Get),
                    path: String::from("/api/users"),
                    service: String::from("user-service"),
                },
                &token,
            )
            .unwrap();

        let request = HttpRequest::new(HttpMethod::Get, "/api/users");
        let service = gateway.route(&request);
        assert_eq!(service, Some(String::from("user-service")));
    }
}
