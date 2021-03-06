/*
 * Omnishock: Something to do with game controllers!
 * Copyright (C) 2017-2019 Jessica Stokes
 *
 * This file is part of Omnishock.
 *
 * Omnishock is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * Omnishock is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with Omnishock.  If not, see <https://www.gnu.org/licenses/>.
 */

extern crate sdl2;
use std::collections::HashMap;

#[cfg(feature = "flamegraph-profiling")]
extern crate flame;

// SDL Manager
// Structure for passing around access to the SDL Subsystems,
// and central place for setting up defaults

pub trait GameController {
    fn name(&self) -> String;
    fn button(&self, button: sdl2::controller::Button) -> bool;
    fn axis(&self, axis: sdl2::controller::Axis) -> i16;
    fn set_rumble(
        &mut self,
        low_frequency_rumble: u16,
        high_frequency_rumble: u16,
        duration_ms: u32,
    ) -> Result<(), String>;
}

pub struct ControllerManager {
    controller: sdl2::controller::GameController,
}

impl GameController for ControllerManager {
    fn name(&self) -> String {
        self.controller.name()
    }

    fn button(&self, button: sdl2::controller::Button) -> bool {
        self.controller.button(button)
    }

    fn axis(&self, axis: sdl2::controller::Axis) -> i16 {
        self.controller.axis(axis)
    }

    fn set_rumble(
        &mut self,
        low_frequency_rumble: u16,
        high_frequency_rumble: u16,
        duration_ms: u32,
    ) -> Result<(), String> {
        match self
            .controller
            .set_rumble(low_frequency_rumble, high_frequency_rumble, duration_ms)
        {
            Ok(_) => Ok(()),
            Err(error) => match error {
                sdl2::IntegerOrSdlError::SdlError(string) => Err(string),
                _ => Err("SDL gave an integer error while setting rumble. WTF?".to_string()),
            },
        }
    }
}

pub struct SDLManager {
    pub context: sdl2::Sdl,
    pub video_subsystem: Option<sdl2::VideoSubsystem>,
    pub game_controller_subsystem: sdl2::GameControllerSubsystem,
    pub active_controllers: HashMap<u32, ControllerManager>,
}

impl SDLManager {
    pub fn init() -> Result<SDLManager, String> {
        #[cfg(feature = "flamegraph-profiling")]
        let _guard = flame::start_guard("SDLManager::init()");
        // Initialise SDL2, plus the video & game controller subsystems
        let context = {
            #[cfg(feature = "flamegraph-profiling")]
            let _guard = flame::start_guard("initialise sdl2 core");
            sdl2::init()?
        };
        /* NOTE: The video subsystem is not currently used, except for the side
         *       effect that it prevents the system from triggering the screen
         *       saver. It will, however, be used to provide a window for focus
         *       in future. */
        let video_subsystem = {
            #[cfg(feature = "flamegraph-profiling")]
            let _guard = flame::start_guard("initialise video subsystem");
            match context.video() {
                Ok(video) => Some(video),
                Err(error) => {
                    println!("couldn't initialise video: {}", error);
                    None
                }
            }
        };
        let game_controller_subsystem = {
            #[cfg(feature = "flamegraph-profiling")]
            let _guard = flame::start_guard("initialise controller subsystem");
            context.game_controller()?
        };

        // Keep track of the controllers we know of
        let active_controllers: HashMap<u32, ControllerManager> = HashMap::new();

        let mut sdl_manager = SDLManager {
            context,
            video_subsystem,
            game_controller_subsystem,
            active_controllers,
        };

        #[cfg(feature = "flamegraph-profiling")]
        flame::start("import controller mappings");
        // Load pre-set controller mappings (note that SDL will still read
        // others from the SDL_GAMECONTROLLERCONFIG environment variable)
        let controller_mappings =
            include_str!("../vendor/SDL_GameControllerDB/gamecontrollerdb.txt")
                .lines()
                .map(|line| line.trim())
                .filter(|line| !line.is_empty() && !line.starts_with('#'));

        // Load each mapping individually rather than using load_mappings,
        // as it turns out doing them together can break without warning
        // if the file's syntax is ever invalid
        for mapping in controller_mappings {
            if let Err(error) = sdl_manager.game_controller_subsystem.add_mapping(mapping) {
                panic!("failed to load mapping: {}", error)
            }
        }
        #[cfg(feature = "flamegraph-profiling")]
        flame::end("import controller mappings");

        // Look into controllers that were already connected at start-up
        sdl_manager.add_available_controllers();

        Ok(sdl_manager)
    }

    fn add_available_controllers(&mut self) {
        #[cfg(feature = "flamegraph-profiling")]
        let _guard = flame::start_guard("SDLManager#add_available_controllers()");
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
                    println!(
                        "Note: joystick {} can't be used as a controller: {}",
                        index, error
                    );
                }
            };
        }
    }

    fn insert_controller(&mut self, index: u32) -> Result<u32, sdl2::IntegerOrSdlError> {
        #[cfg(feature = "flamegraph-profiling")]
        let _guard = flame::start_guard("SDLManager#insert_controller()");
        let controller = self.game_controller_subsystem.open(index)?;
        let controller_id = controller.instance_id();

        let controller_manager = ControllerManager { controller };

        self.active_controllers
            .insert(controller_id, controller_manager);
        Ok(controller_id)
    }

    pub fn add_controller(&mut self, index: u32) -> Result<u32, sdl2::IntegerOrSdlError> {
        #[cfg(feature = "flamegraph-profiling")]
        let _guard = flame::start_guard("SDLManager#add_controller()");
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

        result
    }

    pub fn has_controller(&self, index: u32) -> Result<bool, sdl2::IntegerOrSdlError> {
        #[cfg(feature = "flamegraph-profiling")]
        let _guard = flame::start_guard("SDLManager#has_controller()");
        let controller = self.game_controller_subsystem.open(index)?;
        Ok(self
            .active_controllers
            .contains_key(&controller.instance_id()))
    }

    pub fn remove_controller(&mut self, id: u32) -> Option<ControllerManager> {
        #[cfg(feature = "flamegraph-profiling")]
        let _guard = flame::start_guard("SDLManager#remove_controller()");
        match self.active_controllers.remove(&id) {
            Some(controller_manager) => {
                println!(
                    "Removed “{}” (#{})",
                    controller_manager.controller.name(),
                    id
                );

                Some(controller_manager)
            }
            None => None,
        }
    }
}
