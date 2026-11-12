use deckforge_common::DECK_COUNT;
use deckforge_engine::Command;
use deckforge_library::Library;

use crate::commands::UiCommand;

const HARDWARE_PHONES: &str = "audio.hardwarePhones";
const CROSSFADER_CURVE: &str = "mixer.crossfaderCurve";
const EQ_CURVE: &str = "mixer.eqCurve";
const KEY_LOCK: &str = "deck.keyLock";

/// Saves the settings that survive a restart when the UI changes them.
pub fn remember(library: &Library, cmd: &UiCommand) {
    let _ = match cmd {
        UiCommand::HardwarePhones { enabled } => library.set_setting(HARDWARE_PHONES, enabled),
        UiCommand::CrossfaderCurve { curve } => library.set_setting(CROSSFADER_CURVE, curve),
        UiCommand::EqCurve { curve } => library.set_setting(EQ_CURVE, curve),
        UiCommand::SetKeyLock { deck, enabled } if *deck < DECK_COUNT => {
            let mut locks = library.setting::<[bool; DECK_COUNT]>(KEY_LOCK).unwrap_or_default();
            locks[*deck] = *enabled;
            library.set_setting(KEY_LOCK, &locks)
        }
        _ => Ok(()),
    };
}

/// Engine commands that put the remembered settings back at startup.
pub fn restore(library: &Library) -> Vec<Command> {
    let mut commands = Vec::new();
    if let Some(hw) = library.setting(HARDWARE_PHONES) {
        commands.push(Command::HardwarePhones(hw));
    }
    if let Some(curve) = library.setting(CROSSFADER_CURVE) {
        commands.push(Command::CrossfaderCurve(curve));
    }
    if let Some(curve) = library.setting(EQ_CURVE) {
        commands.push(Command::EqCurve(curve));
    }
    if let Some(locks) = library.setting::<[bool; DECK_COUNT]>(KEY_LOCK) {
        commands.extend(locks.into_iter().enumerate().map(|(deck, enabled)| Command::SetKeyLock { deck, enabled }));
    }
    commands
}

pub fn hardware_phones_saved(library: &Library) -> bool {
    library.setting::<bool>(HARDWARE_PHONES).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckforge_common::CrossfaderCurve;

    fn ui(json: &str) -> UiCommand {
        serde_json::from_str(json).unwrap()
    }

    fn key_locks(commands: &[Command]) -> Vec<(usize, bool)> {
        commands.iter().filter_map(|c| if let Command::SetKeyLock { deck, enabled } = c { Some((*deck, *enabled)) } else { None }).collect()
    }

    #[test]
    fn key_lock_is_remembered_per_deck() {
        let lib = Library::open_in_memory().unwrap();
        assert!(key_locks(&restore(&lib)).is_empty(), "nothing restored before anything is set");
        remember(&lib, &ui(r#"{"type":"setKeyLock","deck":1,"enabled":true}"#));
        assert_eq!(key_locks(&restore(&lib)), vec![(0, false), (1, true)]);
        remember(&lib, &ui(r#"{"type":"setKeyLock","deck":0,"enabled":true}"#));
        remember(&lib, &ui(r#"{"type":"setKeyLock","deck":1,"enabled":false}"#));
        assert_eq!(key_locks(&restore(&lib)), vec![(0, true), (1, false)]);
    }

    #[test]
    fn crossfader_off_is_remembered() {
        let lib = Library::open_in_memory().unwrap();
        remember(&lib, &ui(r#"{"type":"crossfaderCurve","curve":"off"}"#));
        assert!(restore(&lib).iter().any(|c| matches!(c, Command::CrossfaderCurve(CrossfaderCurve::Off))));
    }
}
