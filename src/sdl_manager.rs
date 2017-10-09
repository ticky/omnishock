extern crate sdl2;
use std::collections::HashMap;

// SDL Manager
// Structure for passing around access to the SDL Subsystems,
// and central place for setting up defaults

pub struct SDLManager {
    pub context: sdl2::Sdl,
    pub game_controller_subsystem: sdl2::GameControllerSubsystem,
    pub haptic_subsystem: sdl2::HapticSubsystem,
    pub active_controllers: HashMap<i32, sdl2::controller::GameController>,
}

impl SDLManager {
    pub fn init() -> SDLManager {
        // Initialise SDL2, plus the haptic and game controller subsystems
        let context = sdl2::init().unwrap();
        let haptic_subsystem = context.haptic().unwrap();
        let game_controller_subsystem = context.game_controller().unwrap();

        // Keep track of the controllers we know of
        let mut active_controllers: HashMap<i32, sdl2::controller::GameController> = HashMap::new();

        let mut sdl_manager = SDLManager {
            context,
            game_controller_subsystem,
            haptic_subsystem,
            active_controllers,
        };

        // Load pre-set controller mappings (note that SDL will still read
        // others from the SDL_GAMECONTROLLERCONFIG environment variable)
        let controller_mappings = include_str!(
            "../vendor/SDL_GameControllerDB/gamecontrollerdb.txt"
        ).lines()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty() && !line.starts_with('#'));

        // Load each mapping individually rather than using load_mappings,
        // as it turns out doing them together can break without warning
        // if the file's syntax is ever invalid
        for mapping in controller_mappings {
            match sdl_manager.game_controller_subsystem.add_mapping(mapping) {
                Err(error) => panic!("failed to load mapping: {}", error),
                _ => (),
            }
        }

        // Look into controllers that were already connected at start-up
        sdl_manager.add_available_controllers();

        return sdl_manager;
    }

    pub fn add_available_controllers(&mut self) {
        let joystick_count = match self.game_controller_subsystem.num_joysticks() {
            Ok(count) => count,
            Err(error) => panic!("failed to enumerate joysticks: {}", error),
        };

        for index in 0..joystick_count {
            if self.game_controller_subsystem.is_game_controller(index) {
                match self.game_controller_subsystem.open(index) {
                    Ok(controller) => {
                        let controller_id = &controller.instance_id();
                        println!("{} (#{}): found", controller.name(), controller_id);
                        self.active_controllers.insert(*controller_id, controller);
                    }
                    Err(error) => {
                        println!(
                            "could not initialise joystick {} as controller: {:?}",
                            index,
                            error
                        )
                    }
                }
            }
        }
    }
}
