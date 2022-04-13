use actix_web::{web::Data, App, HttpServer};

mod managers;
mod models;
mod web;

struct AppData {}

/// Start the web server
#[actix_web::main]
async fn main() -> std::io::Result<()> {
	HttpServer::new(|| {
		App::new()
			.app_data(Data::new(AppData {}))
			.service(web::module())
	})
	.bind(("127.0.0.1", 8080))?
	.run()
	.await
}
