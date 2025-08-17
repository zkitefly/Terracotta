pub fn configure(rocket: rocket::Rocket<rocket::Build>) -> rocket::Rocket<rocket::Build> {
    rocket.attach(rocket::fairing::AdHoc::on_response(
        "Authlib-Injector API Location Indication",
        |_, response| {
            Box::pin(async move {
                response.set_header(rocket::http::Header::new(
                    "X-Authlib-Injector-API-Location",
                    "/yggdrasil/",
                ));
            })
        },
    ))
}
