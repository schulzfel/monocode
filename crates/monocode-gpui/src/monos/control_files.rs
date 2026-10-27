//! A Mono's memory, soul and habits as its own operations change them: the
//! same core functions in the same order as the old bridge's actions, so
//! the files, their limits and their words are the bridge's.
//! Each runs on a worker, as the bridge's did.
use monocode_control::{
    Command,
    ops::{
        mono_habit::{
            HabitSchedule, MAX_HABIT_ID_BYTES, MAX_HABIT_INSTRUCTIONS_BYTES, MAX_HABIT_NAME_BYTES,
            MAX_HABIT_NAME_CHARS, MonoHabitUpdateInput,
        },
        mono_memory::{MAX_FACT_BYTES, MAX_QUERY_BYTES},
        mono_soul::{MAX_HASH_BYTES, MAX_SOUL_BYTES},
    },
};
use monocode_core::{
    mono::{
        files,
        habits::{self, Habit},
        memory,
    },
    time::now_ms,
};
use serde_json::{Value, json};
use std::path::Path;

/// A habit that is not there, in the bridge's words.
pub(super) const HABIT_GONE: &str = "Habit not found";
/// The soul's file in the Mono's folder.
const SOUL: &str = "SOUL.md";

/// `value` when it is not blank and fits `max` bytes, else the bridge's
/// "Provide {key} (up to {max} bytes)".
pub(super) fn text(value: &str, key: &str, max: usize) -> Result<String, String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("Provide {key} (up to {max} bytes)"));
    }
    Ok(value.to_owned())
}

fn json<T: serde::Serialize>(value: &T) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|e| e.to_string())
}

/// A schedule as the habit files keep it, with their defaults for what it
/// leaves out.
fn schedule(schedule: &HabitSchedule) -> Result<habits::Schedule, String> {
    serde_json::from_value(json(schedule)?).map_err(|e| e.to_string())
}

/// Runs `command`, one of a Mono's memory, soul or habit operations, on
/// Mono `mono`'s files under `root`.
pub(super) fn run(root: &Path, mono: &str, command: Command) -> Result<Value, String> {
    match command {
        Command::MonoMemoryRead(input) => json(&memory::read(root, mono, input.topic.as_deref())?),
        Command::MonoMemorySearch(input) => {
            let query = text(&input.query, "query", MAX_QUERY_BYTES)?;
            let hits = memory::search(root, mono, &query, input.since.as_deref())?;
            Ok(json!({ "hits": hits }))
        }
        Command::MonoMemoryAdd(input) => {
            let entry = entry(&input.fact, input.until.as_deref())?;
            let topic = input.topic.as_deref();
            let hash = memory::edit(root, mono, topic, |old| Ok(memory::add(old, &entry)))?;
            Ok(json!({ "hash": hash }))
        }
        Command::MonoMemoryReplace(input) => {
            let entry = entry(&input.fact, input.until.as_deref())?;
            let topic = input.topic.as_deref();
            let hash = memory::edit(root, mono, topic, |old| {
                memory::replace(old, &input.find, Some(&entry), &memory::date())
            })?;
            Ok(json!({ "hash": hash }))
        }
        Command::MonoMemoryRemove(input) => {
            let topic = input.topic.as_deref();
            let hash = memory::edit(root, mono, topic, |old| {
                memory::replace(old, &input.find, None, &memory::date())
            })?;
            Ok(json!({ "hash": hash }))
        }
        Command::MonoSoulRead(_) => json(&files::read(root, mono, None, SOUL)?),
        Command::MonoSoulUpdate(input) => {
            if input.text.len() > MAX_SOUL_BYTES {
                return Err("Provide text".into());
            }
            let expected = text(&input.expected_hash, "expectedHash", MAX_HASH_BYTES)?;
            let hash = files::save(root, mono, None, SOUL, &input.text, Some(&expected))?;
            Ok(json!({ "hash": hash }))
        }
        Command::MonoHabitList(_) => Ok(json!({ "habits": habits::load(root, mono)? })),
        Command::MonoHabitAdd(input) => {
            let schedule = schedule(&input.schedule)?;
            let habit = Habit::new(
                text(&input.name, "name", MAX_HABIT_NAME_BYTES)?,
                text(
                    &input.instructions,
                    "instructions",
                    MAX_HABIT_INSTRUCTIONS_BYTES,
                )?,
                schedule,
                now_ms(),
            )?;
            habits::update(root, mono, |all| {
                all.push(habit.clone());
                Ok(())
            })?;
            json(&habit)
        }
        Command::MonoHabitUpdate(input) => {
            let id = text(&input.id, "id", MAX_HABIT_ID_BYTES)?;
            habits::update(root, mono, |all| {
                let habit = all
                    .iter_mut()
                    .find(|h| h.id == id)
                    .ok_or(HABIT_GONE.to_owned())?;
                update(habit, &input)?;
                json(habit)
            })
        }
        Command::MonoHabitRun(input) => {
            let id = text(&input.id, "id", MAX_HABIT_ID_BYTES)?;
            habits::update(root, mono, |all| {
                let habit = all
                    .iter_mut()
                    .find(|h| h.id == id)
                    .ok_or(HABIT_GONE.to_owned())?;
                habit.run_requested = true;
                json(habit)
            })
        }
        Command::MonoHabitRemove(input) => {
            let id = text(&input.id, "id", MAX_HABIT_ID_BYTES)?;
            habits::update(root, mono, |all| {
                let index = all
                    .iter()
                    .position(|h| h.id == id)
                    .ok_or(HABIT_GONE.to_owned())?;
                all.remove(index);
                Ok(json!({ "removed": true }))
            })
        }
        other => Err(format!("{} is not about a Mono's files", other.name())),
    }
}

/// A memory entry for `fact`, dated today: at most 4,000 bytes as given,
/// then the core's own rules.
fn entry(fact: &str, until: Option<&str>) -> Result<String, String> {
    memory::entry(&text(fact, "fact", MAX_FACT_BYTES)?, &memory::date(), until)
}

/// `input`'s changes to `habit`, checked in the bridge's order and words; a
/// new schedule, or turning it on, works out its next run again.
fn update(habit: &mut Habit, input: &MonoHabitUpdateInput) -> Result<(), String> {
    if let Some(name) = &input.name {
        if name.trim().is_empty() || name.chars().count() > MAX_HABIT_NAME_CHARS {
            return Err("Invalid habit name".into());
        }
        habit.name = name.trim().into();
    }
    if let Some(instructions) = &input.instructions {
        if instructions.trim().is_empty() || instructions.len() > MAX_HABIT_INSTRUCTIONS_BYTES {
            return Err("Invalid habit instructions".into());
        }
        habit.instructions.clone_from(instructions);
    }
    if let Some(next) = &input.schedule {
        habit.schedule = schedule(next)?;
        habit.schedule.validate()?;
        habit.next_run_at = habit.schedule.next(now_ms())?;
    }
    if let Some(enabled) = input.enabled {
        habit.enabled = enabled;
        if enabled {
            habit.next_run_at = habit.schedule.next(now_ms())?;
        }
    }
    Ok(())
}
