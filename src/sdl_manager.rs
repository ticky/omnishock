extern crate sdl2;
use std::collections::HashMap;

// SDL Manager
// Structure for passing around access to the SDL Subsystems,
// and central place for setting up defaults

pub struct ControllerManager {
    pub controller: sdl2::controller::GameController,
    pub haptic: Option<sdl2::haptic::Haptic>
}

pub struct SDLManager {
    pub context: sdl2::Sdl,
    pub game_controller_subsystem: sdl2::GameControllerSubsystem,
    pub haptic_subsystem: sdl2::HapticSubsystem,
    pub active_controllers: HashMap<i32, ControllerManager>,
}

impl SDLManager {
    pub fn init() -> SDLManager {
        // Initialise SDL2, plus the haptic and game controller subsystems
        let context = sdl2::init().unwrap();
        let haptic_subsystem = context.haptic().unwrap();
        let game_controller_subsystem = context.game_controller().unwrap();

        // Keep track of the controllers we know of
        let active_controllers: HashMap<i32, ControllerManager> = HashMap::new();

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

    fn add_available_controllers(&mut self) {
        let joystick_count = match self.game_controller_subsystem.num_joysticks() {
            Ok(count) => count,
            Err(error) => panic!("failed to enumerate joysticks: {}", error),
        };

        for index in 0..joystick_count {
            match self.insert_controller(index) {
                Ok(controller_id) => {
                    println!(
                        "Found “{}” (#{})",
                        self.active_controllers[&controller_id].controller.name(),
                        controller_id
                    );
                }
                Err(error) => {
                    println!("Note: joystick {} can't be used as a controller: {}", index, error);
                }
            };
        }
    }

    fn insert_controller(&mut self, index: u32) -> Result<i32, sdl2::IntegerOrSdlError> {
        let controller = self.game_controller_subsystem.open(index)?;
        let haptic = self.haptic_subsystem.open_from_joystick_id(index as i32).ok();
        let controller_id = controller.instance_id();

        match haptic {
            None => {
                println!("Note: “{}” (#{}) doesn't support haptic feedback.", controller.name(), controller_id);
            }
            _ => ()
        }

        let controller_manager = ControllerManager {
            controller,
            haptic
        };

        self.active_controllers.insert(controller_id, controller_manager);
        Ok(controller_id)
    }

    pub fn add_controller(&mut self, index: u32) -> Result<i32, sdl2::IntegerOrSdlError> {
        let controller = self.game_controller_subsystem.open(index)?;
        let controller_id = controller.instance_id();

        if self.active_controllers.contains_key(&controller_id) {
            return Ok(controller_id);
        }

        let result = self.insert_controller(index);

        println!(
            "Added “{}” (#{})",
            self.active_controllers[&controller_id].controller.name(),
            controller_id
        );

        return result;
    }

    pub fn has_controller(&self, index: u32) -> Result<bool, sdl2::IntegerOrSdlError> {
        let controller = self.game_controller_subsystem.open(index)?;
        return Ok(self.active_controllers.contains_key(&controller.instance_id()));
    }

    pub fn remove_controller(&mut self, id: i32) -> Option<ControllerManager> {
        return match self.active_controllers.remove(&id) {
            Some(controller_manager) => {
                println!(
                    "Removed “{}” (#{})",
                    controller_manager.controller.name(),
                    id
                );

                return Some(controller_manager)
            },
            None => None
        };
    }
}
