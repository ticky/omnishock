extern crate sdl2;

fn main() {
    let sdl_context = sdl2::init().unwrap();
    let game_controller_subsystem = sdl_context.game_controller().unwrap();

    match game_controller_subsystem.load_mappings("/Users/jessica/Repositories/SDL_GameControllerDB/gamecontrollerdb.txt") {
        Err(error) => panic!("can't load mappings: {}", error),
        _ => (),
    };

    // TODO: this should be a hashmap of instance_id to gamecontroller!
    let mut controllers: Vec<sdl2::controller::GameController> = Vec::new();

    for event in sdl_context.event_pump().unwrap().wait_iter() {
        use sdl2::event::Event;

        match event {
            Event::ControllerDeviceAdded{ which, .. } => {
                match game_controller_subsystem.open(which as u32) {
                    Ok(controller) => {
                        println!("{} connected as #{}! (joystick ID is {})", controller.name(), controller.instance_id(), which);
                        controllers.push(controller);
                    },
                    Err(error) => println!("could not initialise connected controller #{}: {:?}", which, error),
                }
            },

            Event::ControllerAxisMotion{ which, axis, value, .. } => {
                println!("Controller {} axis {:?} moved to {}", which, axis, value);
            },

            Event::ControllerButtonDown{ which, button, .. } => {
                println!("Controller {} button {:?} down", which, button);
            },

            Event::ControllerButtonUp{ which, button, .. } => {
                println!("Controller {} button {:?} up", which, button);
            },

            Event::ControllerDeviceRemoved{ which, .. } => {
                println!("Controller {} disconnected!", which);
            },

            Event::ControllerDeviceRemapped{ which, .. } => {
                println!("Controller {} remapped!", which);
            },

            Event::Quit{..} => break,
            _ => (),
        }
    }
}
