//! MIDI input via midir.

use dx7_core::SynthCommand;
use midir::{MidiInput, MidiInputConnection};
use ringbuf::traits::Producer;
use std::sync::{Arc, Mutex};

/// MIDI input handler.
pub struct MidiHandler {
    _connection: MidiInputConnection<()>,
    pub port_name: String,
}

impl MidiHandler {
    /// Try to connect to a MIDI input port.
    /// Uses the shared command producer from the audio engine.
    pub fn connect(
        port_name: Option<&str>,
        command_tx: Arc<Mutex<ringbuf::HeapProd<SynthCommand>>>,
    ) -> Result<Self, String> {
        let midi_in = MidiInput::new("dx7-app")
            .map_err(|e| format!("Failed to create MIDI input: {e}"))?;

        let ports = midi_in.ports();
        if ports.is_empty() {
            return Err("No MIDI input ports available".into());
        }

        // Find the requested port or use the first one
        let (port_idx, name) = if let Some(requested) = port_name {
            let found = ports.iter().enumerate().find(|(_, p)| {
                midi_in
                    .port_name(p)
                    .map(|n| n.contains(requested))
                    .unwrap_or(false)
            });
            match found {
                Some((idx, p)) => (idx, midi_in.port_name(p).unwrap_or_default()),
                None => return Err(format!("MIDI port '{}' not found", requested)),
            }
        } else {
            (0, midi_in.port_name(&ports[0]).unwrap_or_default())
        };

        let port = &ports[port_idx];
        let port_name_str = name.clone();

        let connection = midi_in
            .connect(
                port,
                "dx7-input",
                move |_stamp, message, _| {
                    if let Some(cmd) = parse_midi_message(message) {
                        if let Ok(mut tx) = command_tx.lock() {
                            let _ = tx.try_push(cmd);
                        }
                    }
                },
                (),
            )
            .map_err(|e| format!("Failed to connect to MIDI port: {e}"))?;

        Ok(MidiHandler {
            _connection: connection,
            port_name: port_name_str,
        })
    }

    /// List available MIDI input ports.
    pub fn list_ports() -> Vec<String> {
        let midi_in = match MidiInput::new("dx7-list") {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };

        midi_in
            .ports()
            .iter()
            .filter_map(|p| midi_in.port_name(p).ok())
            .collect()
    }
}

/// Parse a raw MIDI message into a SynthCommand.
fn parse_midi_message(msg: &[u8]) -> Option<SynthCommand> {
    if msg.is_empty() {
        return None;
    }

    let status = msg[0] & 0xF0;
    match status {
        0x90 if msg.len() >= 3 => {
            if msg[2] == 0 {
                Some(SynthCommand::NoteOff { note: msg[1] })
            } else {
                Some(SynthCommand::NoteOn {
                    note: msg[1],
                    velocity: msg[2],
                })
            }
        }
        0x80 if msg.len() >= 3 => Some(SynthCommand::NoteOff { note: msg[1] }),
        0xB0 if msg.len() >= 3 => Some(SynthCommand::ControlChange {
            cc: msg[1],
            value: msg[2],
        }),
        0xE0 if msg.len() >= 3 => {
            let bend = ((msg[2] as i16) << 7 | msg[1] as i16) - 8192;
            Some(SynthCommand::PitchBend { value: bend })
        }
        _ => None,
    }
}
