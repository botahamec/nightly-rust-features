use actix_web::{get, web, Responder, Scope};

#[get("/")]
async fn index() -> impl Responder {
	"Hello, world!"
}

/// The front-end scope for the web app
pub fn module() -> Scope {
	web::scope("/").service(index)
}
