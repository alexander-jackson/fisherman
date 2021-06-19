use std::fmt;

use actix_web::{body::Body, http::StatusCode, HttpResponse, ResponseError};

#[derive(Copy, Clone, Debug)]
pub enum ServerError {
    BadRequest,
    Unauthorized,
    UnprocessableEntity,
}

impl fmt::Display for ServerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::BadRequest => "Bad Request",
            Self::Unauthorized => "Unauthorized",
            Self::UnprocessableEntity => "Unprocessable Entity",
        };

        write!(f, "{}", message)
    }
}

impl ResponseError for ServerError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::UnprocessableEntity => StatusCode::UNPROCESSABLE_ENTITY,
        }
    }

    fn error_response(&self) -> HttpResponse<Body> {
        HttpResponse::build(self.status_code()).body(self.to_string())
    }
}
