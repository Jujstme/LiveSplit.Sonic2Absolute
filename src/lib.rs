#![no_std]
use asr::{signature::Signature, timer, timer::TimerState, watcher::Watcher, Address, Process};

#[cfg(all(not(test), target_arch = "wasm32"))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

static AUTOSPLITTER: spinning_top::Spinlock<State> = spinning_top::const_spinlock(State {
    game: None,
    watchers: Watchers {
        state: Watcher::new(),
        levelid: Watcher::new(),
        startindicator: Watcher::new(),
        zoneselectongamecomplete: Watcher::new(),
        zoneindicator: Watcher::new(),
    },
    // settings: None,
});

struct State {
    game: Option<ProcessInfo>,
    watchers: Watchers,
    // settings: Settings,
}

struct ProcessInfo {
    game: Process,
    main_module_base: Address,
    main_module_size: u64,
    addresses: Option<MemoryPtr>,
}

struct Watchers {
    state: Watcher<u8>,
    levelid: Watcher<Acts>,
    startindicator: Watcher<u8>,
    zoneselectongamecomplete: Watcher<u8>,
    zoneindicator: Watcher<ZoneIndicator>,
}

struct MemoryPtr {
    state: Address,
    levelid: Address,
    startindicator: Address,
    zoneselectongamecomplete: Address,
    zoneindicator: Address,
}

impl State {
    fn attach_process() -> Option<ProcessInfo> {
        const PROCESS_NAMES: [&str; 1] = ["Sonic2Absolute.exe"];
        let mut proc: Option<Process> = None;
        let mut proc_name: &str = "";
    
        for name in PROCESS_NAMES {
            proc = Process::attach(name);
            if proc.is_some() {
                proc_name = name;
                break;
            }
        }
    
        let game = proc?;
        let main_module_base = game.get_module_address(proc_name).ok()?;
        let main_module_size: u64 = 0x4475000; // Hack, until we can actually query ModuleMemorySize
    
        Some(ProcessInfo {
            game,
            main_module_base,
            main_module_size,
            addresses: None,
        })
    }

    fn update(&mut self) {
        // Checks is LiveSplit is currently attached to a target process and runs attach_process() otherwise
        if self.game.is_none() {
            self.game = State::attach_process()
        }
        let Some(game) = &mut self.game else { return };

        if !game.game.is_open() {
            self.game = None;
            return;
        }

        // Get memory addresses
        let Some(addresses) = &game.addresses else { game.addresses = MemoryPtr::new(&game.game, game.main_module_base, game.main_module_size); return; };

        // Update the watchers variables
        let game = &game.game;
        update_internal(game, addresses, &mut self.watchers);

        let timer_state = timer::state();
        if timer_state == TimerState::Running || timer_state == TimerState::Paused {
            /*
            if is_loading(self) {
                timer::pause_game_time()
            } else {
                timer::resume_game_time()
            }
            */

            // timer::set_game_time(game_time());
            if reset(self) {
                timer::reset()
            } else if split(self) {
                timer::split()
            }
            
        } 

        if timer_state == TimerState::NotRunning {
            if start(self) {
                timer::start();
            }
        }     
    }    
}

impl MemoryPtr {
    fn new(process: &Process, addr: Address, size: u64) -> Option<Self> {
        fn pointerpath(process: &Process, ptr: Address, offset1: u32, offset2: u32, offset3: u32) -> Option<Address> {
            let result = process.read_pointer_path32::<u32>(ptr.0 as u32, &[offset1, offset2]).ok()?;
            Some(Address(result as u64 + offset3 as u64))
        }

        const SIG: Signature<19> = Signature::new("3D ???????? 0F 87 ???????? FF 24 85 ???????? A1");
        let ptr = SIG.scan_process_range(process, addr, size)?.0 + 14;
        let ptr = Address(process.read::<u32>(Address(ptr)).ok()? as u64);
        let state = pointerpath(process, ptr, 0x4 * 89, 8, 0x9D8)?;
        let levelid = pointerpath(process, ptr, 0x4 * 123, 1, 0)?;
        let startindicator = pointerpath(process, ptr, 0x4 * 30, 8, 0x9D8)?;
        let zoneselectongamecomplete = pointerpath(process, ptr, 0x4  * 91, 8, 0x9D8)?;
        
        const SIG2: Signature<11> = Signature::new("69 F8 ???????? B8 ????????");
        let ptr = SIG2.scan_process_range(process, addr, size)?.0 + 7;
        let zoneindicator = Address(process.read::<u32>(Address(ptr)).ok()? as u64);


        Some(Self {
            state,
            levelid,
            startindicator,
            zoneselectongamecomplete,
            zoneindicator,
        })
    }
}

#[no_mangle]
pub extern "C" fn update() {
    AUTOSPLITTER.lock().update();
}

fn update_internal(game: &Process, addresses: &MemoryPtr, watchers: &mut Watchers) {
    let Some(_thing) = watchers.state.update(game.read(addresses.state).ok()) else { return };
    let Some(_thing) = watchers.startindicator.update(game.read(addresses.startindicator).ok()) else { return };
    let Some(_thing) = watchers.zoneselectongamecomplete.update(game.read(addresses.zoneselectongamecomplete).ok()) else { return };

    let Some(g) = game.read::<u32>(addresses.zoneindicator).ok() else { return };
    let i: ZoneIndicator; 
    match g {
        0x6E69614D => { i = ZoneIndicator::MainMenu },
        0x656E6F5A => { i = ZoneIndicator::Zones },
        0x69646E45 => { i = ZoneIndicator::Ending },
        0x65766153 => { i = ZoneIndicator::SaveSelect },
        _ => { i = ZoneIndicator::Default }
    }
    watchers.zoneindicator.update(Some(i));

    if watchers.zoneindicator.pair.unwrap().current == ZoneIndicator::Ending {
        watchers.levelid.update(Some(Acts::Default));
    } else {
        let Some(g) = game.read(addresses.levelid).ok() else { return };
        let i: Acts;
        match g {
            0 => { i = Acts::EmeraldHill1 },
            1 => { i = Acts::EmeraldHill2 },
            2 => { i = Acts::ChemicalPlant1 },
            3 => { i = Acts::ChemicalPlant2 },
            4 => { i = Acts::AquaticRuin1 },
            5 => { i = Acts::AquaticRuin2 },
            6 => { i = Acts::CasinoNight1 },
            7 => { i = Acts::CasinoNight2 },
            8 => { i = Acts::HillTop1 },
            9 => { i = Acts::HillTop2 },
            10 => { i = Acts::MysticCave1 },
            11 => { i = Acts::MysticCave2 },
            12 => { i = Acts::OilOcean1 },
            13 => { i = Acts::OilOcean2 },
            14 => { i = Acts::Metropolis1 },
            15 => { i = Acts::Metropolis2 },
            16 => { i = Acts::Metropolis3 },
            17 => { i = Acts::SkyChase },
            18 => { i = Acts::WingFortress },
            19 => { i = Acts::DeathEgg },
            _ => { i = Acts::Default },
        }
        watchers.levelid.update(Some(i));
    }
}

fn start(state: &State) -> bool {
    let Some(state2) = &state.watchers.state.pair else { return false };
    let Some(startindicator) = &state.watchers.startindicator.pair else { return false };
    let Some(zoneselectongamecomplete) = &state.watchers.zoneselectongamecomplete.pair else { return false };

    let runstartedsavefile = state2.old == 5 && state2.current == 7;
    let ronstartednosavefile = state2.current == 4 && startindicator.changed() && startindicator.current == 1;
    let runstartedngp = state2.current == 6 && startindicator.changed() && startindicator.current == 1 && zoneselectongamecomplete.current == 0;

    runstartedsavefile || ronstartednosavefile || runstartedngp
}

fn split(state: &State) -> bool {
    let Some(levelid) = &state.watchers.levelid.pair else { return false };
    match levelid.current {
        Acts::EmeraldHill2 => { if levelid.old == Acts::EmeraldHill1 { return true } },
        Acts::ChemicalPlant1 => { if levelid.old == Acts::EmeraldHill2 { return true } },
        Acts::ChemicalPlant2 => { if levelid.old == Acts::ChemicalPlant1 { return true } },
        Acts::AquaticRuin1 => { if levelid.old == Acts::ChemicalPlant2 { return true }},
        Acts::AquaticRuin2 => { if levelid.old == Acts::AquaticRuin1 { return true }},
        Acts::CasinoNight1 => { if levelid.old == Acts::AquaticRuin2 { return true }},
        Acts::CasinoNight2 => { if levelid.old == Acts::CasinoNight1 { return true }},
        Acts::HillTop1 => { if levelid.old == Acts::CasinoNight2 { return true }},
        Acts::HillTop2 => { if levelid.old == Acts::HillTop1 { return true }},
        Acts::MysticCave1 => { if levelid.old == Acts::HillTop2 { return true }},
        Acts::MysticCave2 => { if levelid.old == Acts::MysticCave1 { return true }},
        Acts::OilOcean1 => { if levelid.old == Acts::MysticCave2 { return true }},
        Acts::OilOcean2 => { if levelid.old == Acts::OilOcean1 { return true }},
        Acts::Metropolis1 => { if levelid.old == Acts::OilOcean2 { return true }},
        Acts::Metropolis2 => { if levelid.old == Acts::Metropolis1 { return true }},
        Acts::Metropolis3 => { if levelid.old == Acts::Metropolis2 { return true }},
        Acts::SkyChase => { if levelid.old == Acts::Metropolis3 { return true }},
        Acts::WingFortress => { if levelid.old == Acts::SkyChase { return true }},
        Acts::DeathEgg => { if levelid.old == Acts::WingFortress { return true }},
        Acts::Default => { if levelid.old != levelid.current { return true }},
        _ => {},
    }
    false
}

fn reset(state: &State) -> bool {
    let Some(state2) = &state.watchers.state.pair else { return false };
    state2.old == 0 && (state2.current == 4 || state2.current == 5)
}

/*
fn is_loading(state: &State) -> bool {
    false
}
*/

#[derive(Clone, Copy, Eq, PartialEq)]
enum ZoneIndicator {
    MainMenu,
    Zones,
    Ending,
    SaveSelect,
    Default,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Acts {
    EmeraldHill1,
    EmeraldHill2,
    ChemicalPlant1,
    ChemicalPlant2,
    AquaticRuin1,
    AquaticRuin2,
    CasinoNight1,
    CasinoNight2,
    HillTop1,
    HillTop2,
    MysticCave1,
    MysticCave2,
    OilOcean1,
    OilOcean2,
    Metropolis1,
    Metropolis2,
    Metropolis3,
    SkyChase,
    WingFortress,
    DeathEgg,
    Default,
}