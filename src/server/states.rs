use rocket::http::Status;
use rocket::serde::json::Json;
use serde_json::Value;

use crate::code::Room;
use crate::core;

#[get("/")]
fn get_state() -> Json<Value> {
    return Json(core::get_state());
}

#[get("/ide")]
fn set_state_ide() -> Status {
    core::set_waiting();
    return Status::Ok;
}

#[get("/scanning")]
fn set_state_scanning() -> Status {
    core::set_scanning();
    return Status::Ok;
}

#[get("/guesting?<room>")]
fn set_state_guesting(room: Option<String>) -> Status {
    if let Some(room) = room
        && let Some(room) = Room::from(&room)
    {
        core::set_guesting(room);
        return Status::Ok;
    }

    return Status::BadRequest;
}

pub fn configure(rocket: rocket::Rocket<rocket::Build>) -> rocket::Rocket<rocket::Build> {
    rocket.mount(
        "/state",
        routes![
            get_state,
            set_state_ide,
            set_state_scanning,
            set_state_guesting,
        ],
    )
}
