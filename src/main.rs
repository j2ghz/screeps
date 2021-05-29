use log::*;
use screeps::{
    find, game::spawns, prelude::*, CircleStyle, Creep, MoveToOptions, Part, Position,
    ResourceType, ReturnCode, RoomObjectProperties, Terrain,
};
use std::collections::HashSet;
use stdweb::js;

mod logging;

fn main() {
    logging::setup_logging(logging::Debug);

    js! {
        var game_loop = @{game_loop};

        module.exports.loop = function() {
            // Provide actual error traces.
            try {
                game_loop();
            } catch (error) {
                // console_error function provided by 'screeps-game-api'
                console_error("caught exception:", error);
                if (error.stack) {
                    console_error("stack trace:", error.stack);
                }
                console_error("resetting VM next tick.");
                // reset the VM since we don't know if everything was cleaned up and don't
                // want an inconsistent state.
                module.exports.loop = wasm_initialize;
            }
        }
    }
}

fn is_enterable(p: Position) -> bool {
    !p.look().iter().any(|look_result| match look_result {
        screeps::LookResult::Structure(_) => true,
        screeps::LookResult::Terrain(Terrain::Wall) => true,
        _ => false,
    })
}

fn game_loop() {
    debug!("loop starting! CPU: {}", screeps::game::cpu::get_used());

    // for room in screeps::game::rooms::values() {
    //     debug!("inspectiong {}", room.name());

    //     for exit in room.find(find::EXIT) {
    //         debug!("exit at x:{} y:{}", exit.x(), exit.y());
    //         if is_enterable(exit) {
    //             room.visual().circle(
    //                 clamp(exit.x(), 1, 48) as f32,
    //                 clamp(exit.y(), 1, 48) as f32,
    //                 Some(CircleStyle::default()),
    //             )
    //         }
    //     }
    // }

    trace!("running spawns");
    for spawn in screeps::game::spawns::values() {
        trace!("running spawn {}", spawn.name());
        let body = [Part::Move, Part::Move, Part::Carry, Part::Work];

        if spawn.energy() >= body.iter().map(|p| p.cost()).sum() {
            // create a unique name, spawn.
            let name_base = screeps::game::time();
            let mut additional = 0;
            let res = loop {
                let name = format!("{}-{}", name_base, additional);
                let res = spawn.spawn_creep(&body, &name);

                if res == ReturnCode::NameExists {
                    additional += 1;
                } else {
                    break res;
                }
            };

            if res != ReturnCode::Ok {
                warn!("couldn't spawn: {:?}", res);
            }
        }
    }

    trace!("running creeps");
    for creep in screeps::game::creeps::values() {
        let name = creep.name();
        trace!("running creep {}", name);
        if creep.spawning() {
            continue;
        }

        if creep.memory().bool("harvesting") {
            if creep.store_free_capacity(Some(ResourceType::Energy)) == 0 {
                creep.memory().set("harvesting", false);
            }
        } else {
            if creep.store_used_capacity(None) == 0 {
                creep.memory().set("harvesting", true);
            }
        }

        if creep.memory().bool("harvesting") {
            let source = &creep
                .room()
                .expect("room is not visible to you")
                .find(find::SOURCES)[0];
            // let source = &creep
            //     .pos()
            //     .find_closest_by_range(find::SOURCES_ACTIVE)
            //     .expect("couldn't find closest source");
            if creep.pos().is_near_to(source) {
                creep.harvest(source).ok_or_print_warn("couldn't harvest");
            } else {
                goto(&creep, source);
            }
        } else {
            // if let Some(s) = spawns::values().first() {
            //     match creep.transfer_all(s, ResourceType::Energy) {
            //         ReturnCode::Ok => {}
            //         ReturnCode::NotInRange => {
            //             goto(&creep, s);
            //         }
            //         x => info!("couldn't transfer energy: {:?}", x),
            //     }
            // } else
            if let Some(c) = creep
                .room()
                .expect("room is not visible to you")
                .controller()
            {
                let r = creep.upgrade_controller(&c);
                if r == ReturnCode::NotInRange {
                    goto(&creep, &c);
                } else if r != ReturnCode::Ok {
                    warn!("couldn't upgrade: {:?}", r);
                }
            } else {
                warn!("creep room has no controller!");
            }
        }
    }

    let time = screeps::game::time();

    if time % 32 == 3 {
        info!("running memory cleanup");
        cleanup_memory().expect("expected Memory.creeps format to be a regular memory object");
    }

    info!("done! cpu: {}", screeps::game::cpu::get_used())
}

fn clamp<T: std::cmp::PartialOrd>(val: T, min: T, max: T) -> T {
    assert!(min <= max);
    if val < min {
        min
    } else if val > max {
        max
    } else {
        val
    }
}

fn goto<T: RoomObjectProperties + HasPosition>(creep: &Creep, dest: &T) {
    match creep.move_to_with_options(
        dest,
        MoveToOptions::default().visualize_path_style(screeps::PolyStyle::default()),
    ) {
        ReturnCode::Ok => {}
        ReturnCode::Tired => {}  // fine
        ReturnCode::NoPath => {} // not fine, but idk
        other => other.ok_or_print_debug("couldn't move"),
    }
}

trait HandleReturnCode {
    fn ok_or_print_debug(self, message: &str);
    fn ok_or_print_warn(self, message: &str);
}

impl HandleReturnCode for ReturnCode {
    fn ok_or_print_debug(self, message: &str) {
        if self != ReturnCode::Ok {
            debug!("{}: {:?}", message, self);
        }
    }

    fn ok_or_print_warn(self, message: &str) {
        if self != ReturnCode::Ok {
            warn!("{}: {:?}", message, self);
        }
    }
}

trait HasCoordinatesF {
    fn coords_f(&self) -> (f32, f32);
}

impl HasCoordinatesF for Position {
    fn coords_f(&self) -> (f32, f32) {
        let (x, y) = self.coords();
        (x as f32, y as f32)
    }
}

fn cleanup_memory() -> Result<(), Box<dyn std::error::Error>> {
    let alive_creeps: HashSet<String> = screeps::game::creeps::keys().into_iter().collect();

    let screeps_memory = match screeps::memory::root().dict("creeps")? {
        Some(v) => v,
        None => {
            warn!("not cleaning game creep memory: no Memory.creeps dict");
            return Ok(());
        }
    };

    for mem_name in screeps_memory.keys() {
        if !alive_creeps.contains(&mem_name) {
            debug!("cleaning up creep memory of dead creep {}", mem_name);
            screeps_memory.del(&mem_name);
        }
    }

    Ok(())
}
