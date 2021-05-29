use log::*;
use screeps::{
    find, prelude::*, CircleStyle, Creep, Part, Position, ResourceType, ReturnCode,
    RoomObjectProperties, Terrain,
};
use std::collections::HashSet;
use stdweb::js;

mod logging;

fn main() {
    logging::setup_logging(logging::Trace);

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

    for room in screeps::game::rooms::values() {
        debug!("inspectiong {}", room.name());

        for exit in room.find(find::EXIT) {
            debug!("exit at x:{} y:{}", exit.x(), exit.y());
            if is_enterable(exit) {
                room.visual().circle(
                    clamp(exit.x(), 1, 48) as f32,
                    clamp(exit.y(), 1, 48) as f32,
                    Some(CircleStyle::default()),
                )
            }
        }
    }

    debug!("running spawns");
    for spawn in screeps::game::spawns::values() {
        debug!("running spawn {}", spawn.name());
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

    debug!("running creeps");
    for creep in screeps::game::creeps::values() {
        let name = creep.name();
        debug!("running creep {}", name);
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
            if creep.pos().is_near_to(source) {
                let r = creep.harvest(source);
                if r != ReturnCode::Ok {
                    warn!("couldn't harvest: {:?}", r);
                }
            } else {
                goto(&creep, source);
            }
        } else {
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
    if let Some(room) = dest.room() {
        room.visual()
            .line(creep.pos().coords_f(), dest.pos().coords_f(), None)
    }
    creep.move_to(dest);
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
