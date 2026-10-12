#![cfg(not(any(feature = "gen1", feature = "gen2", feature = "gen3", feature = "gen4")))]
//! Encore and 0 PP, against Showdown (gen9championsbssregmc) measured by pokemon_rl
//! scripts/sim_engine_fix/showdown_encore_0pp.js -> experiments/086_encore_pin2_gate/showdown_encore_0pp.json.
//! Showdown's Encore (data/mods/champions/moves.ts, onStart / onResidual):
//!   - fails if the target's last move has no PP left;
//!   - ends at the end of the turn once the encored move has no PP left, freeing the other moves.
//! PP values stay below 10 so the tests hold with and without `exact_pp`. The oracle's Thunderbolt is Dragon
//! Pulse here: Thunderbolt's 10% paralysis can fully paralyze the Encore user, a branch with no Encore at all.

use poke_engine::choices::Choices;
use poke_engine::engine::generate_instructions::generate_instructions_from_move_pair;
use poke_engine::engine::state::{MoveChoice, PokemonVolatileStatus};
use poke_engine::instruction::{Instruction, StateInstructions};
use poke_engine::state::{LastUsedMove, PokemonMoveIndex, State};

/// Side one: Dragon Pulse (M0, `tbolt_pp`) and Ice Punch (M1, 8 PP). Side two: Encore (M0) and Splash (M1),
/// bulky so nothing faints.
fn state(tbolt_pp: i8, side_one_speed: i16) -> State {
    let mut s = State::default();
    s.use_last_used_move = true;
    let u = s.side_one.get_active();
    u.replace_move(PokemonMoveIndex::M0, Choices::DRAGONPULSE);
    u.replace_move(PokemonMoveIndex::M1, Choices::ICEPUNCH);
    u.moves.m0.pp = tbolt_pp;
    u.moves.m1.pp = 8;
    u.moves.m2.pp = 0;
    u.moves.m3.pp = 0;
    u.speed = side_one_speed;
    // no bench, so the option list holds only moves (the default state's placeholders would be switches)
    for i in 1..6 {
        s.side_one.pokemon.pkmn[i].hp = 0;
    }
    let t = s.side_two.get_active();
    t.replace_move(PokemonMoveIndex::M0, Choices::ENCORE);
    t.replace_move(PokemonMoveIndex::M1, Choices::SPLASH);
    t.moves.m0.pp = 8;
    t.moves.m1.pp = 8;
    t.moves.m2.pp = 0;
    t.moves.m3.pp = 0;
    t.speed = 100;
    t.hp = 30000;
    t.maxhp = 30000;
    s
}

fn choice(s: &State, side: u8, name: &str) -> MoveChoice {
    let sd = if side == 1 { &s.side_one } else { &s.side_two };
    MoveChoice::from_string(name, sd).unwrap_or_else(|| panic!("{} not available", name))
}

fn turn(s: &mut State, a: &str, b: &str) -> Vec<StateInstructions> {
    let (c1, c2) = (choice(s, 1, a), choice(s, 2, b));
    let before = format!("{:?}", s);
    let br = generate_instructions_from_move_pair(s, &c1, &c2, false);
    assert_eq!(format!("{:?}", s), before);
    br
}

fn applies_encore(b: &StateInstructions) -> bool {
    b.instruction_list.iter().any(|i| {
        matches!(i, Instruction::ApplyVolatileStatus(v) if v.volatile_status == PokemonVolatileStatus::ENCORE)
    })
}

/// Side one's options, without the tera / mega variants (the gen9 build offers "<move>-tera" too).
fn side_one_options(s: &State) -> Vec<String> {
    s.root_get_all_options()
        .0
        .iter()
        .map(|c| c.to_string(&s.side_one))
        .filter(|o| !o.ends_with("-tera") && !o.ends_with("-mega"))
        .collect()
}

/// Every branch of the turn, applied; `check` sees the resulting state.
fn for_each_outcome(s: &mut State, branches: &[StateInstructions], mut check: impl FnMut(&State)) {
    for b in branches {
        s.apply_instructions(&b.instruction_list);
        check(s);
        s.reverse_instructions(&b.instruction_list);
    }
}

/// Showdown start_fails_last_move_just_hit_0: the faster target spends its last Thunderbolt PP; Encore then
/// fails, and next turn Ice Punch is available.
#[test]
fn test_encore_fails_when_last_move_just_reached_0_pp() {
    let mut s = state(1, 200);
    let br = turn(&mut s, "dragonpulse", "encore");
    assert!(br.iter().all(|b| !applies_encore(b)), "Encore must fail");
    for_each_outcome(&mut s, &br, |after| {
        assert!(!after
            .side_one
            .volatile_statuses
            .contains(&PokemonVolatileStatus::ENCORE));
        assert!(side_one_options(after).contains(&"icepunch".to_string()));
    });
}

/// Showdown start_fails_last_move_0_from_previous_turn: Thunderbolt reached 0 PP last turn; this turn a
/// faster Encore fails, and the target's Ice Punch goes ahead.
#[test]
fn test_encore_fails_when_last_move_is_at_0_pp() {
    let mut s = state(0, 50);
    s.side_one.last_used_move = LastUsedMove::Move(PokemonMoveIndex::M0);
    let br = turn(&mut s, "icepunch", "encore");
    assert!(br.iter().all(|b| !applies_encore(b)), "Encore must fail");
    for_each_outcome(&mut s, &br, |after| {
        assert!(!after
            .side_one
            .volatile_statuses
            .contains(&PokemonVolatileStatus::ENCORE));
        assert_eq!(
            after.side_one.get_active_immutable().moves.m1.pp,
            7,
            "Ice Punch was used"
        );
    });
}

/// Showdown lock_ends_when_encored_move_hits_0: Encore starts on Thunderbolt (2 PP -> 1); next turn the
/// encored Thunderbolt spends its last PP and Encore ends at the end of that turn; Ice Punch is free again.
#[test]
fn test_encore_ends_when_encored_move_reaches_0_pp() {
    let mut s = state(2, 200);
    let br = turn(&mut s, "dragonpulse", "encore");
    assert!(
        br.iter().all(applies_encore),
        "Encore starts (Thunderbolt still has PP)"
    );
    s.apply_instructions(&br[0].instruction_list);
    assert_eq!(
        side_one_options(&s),
        vec!["dragonpulse".to_string()],
        "locked into the encored move"
    );

    let br2 = turn(&mut s, "dragonpulse", "splash");
    for_each_outcome(&mut s, &br2, |after| {
        assert_eq!(after.side_one.get_active_immutable().moves.m0.pp, 0);
        assert!(
            !after
                .side_one
                .volatile_statuses
                .contains(&PokemonVolatileStatus::ENCORE),
            "Encore ended"
        );
        assert_eq!(
            after.side_one.volatile_status_durations.encore, 0,
            "duration counter reset"
        );
        let o = side_one_options(after);
        assert!(
            o.contains(&"icepunch".to_string()) && !o.contains(&"dragonpulse".to_string()),
            "{:?}",
            o
        );
        assert!(!o.contains(&"struggle".to_string()));
    });
}

/// The end-of-turn removal happens only at 0 PP: an encored move with PP left keeps the lock.
#[test]
fn test_encore_lock_kept_while_encored_move_has_pp() {
    let mut s = state(5, 200);
    let br = turn(&mut s, "dragonpulse", "encore");
    s.apply_instructions(&br[0].instruction_list);
    let br2 = turn(&mut s, "dragonpulse", "splash");
    for_each_outcome(&mut s, &br2, |after| {
        assert_eq!(after.side_one.get_active_immutable().moves.m0.pp, 3);
        assert!(after
            .side_one
            .volatile_statuses
            .contains(&PokemonVolatileStatus::ENCORE));
        assert_eq!(side_one_options(after), vec!["dragonpulse".to_string()]);
    });
}

/// Unchanged: Encore with PP left starts, and fails against a Pokemon that has not used a move.
#[test]
fn test_ordinary_encore_unchanged() {
    let mut s = state(8, 200);
    assert!(turn(&mut s, "dragonpulse", "encore")
        .iter()
        .all(applies_encore));
    let mut s = state(8, 50);
    s.side_one.last_used_move = LastUsedMove::Switch(poke_engine::state::PokemonIndex::P0);
    assert!(turn(&mut s, "dragonpulse", "encore")
        .iter()
        .all(|b| !applies_encore(b)));
}

/// An Encore already in place on a 0-PP move when the side chooses (e.g. a hand-built state): the
/// exp085 guard still applies - the side Struggles (feature `struggle`) rather than using the 0-PP move,
/// and the end of that turn removes the Encore.
#[cfg(feature = "struggle")]
#[test]
fn test_struggle_under_a_0_pp_encore_then_encore_ends() {
    let mut s = state(0, 200);
    s.side_one.get_active().moves.m1.pp = 0;
    s.side_one
        .volatile_statuses
        .insert(PokemonVolatileStatus::ENCORE);
    s.side_one.last_used_move = LastUsedMove::Move(PokemonMoveIndex::M0);
    assert!(side_one_options(&s).contains(&"struggle".to_string()));
    let br = turn(&mut s, "struggle", "splash");
    for_each_outcome(&mut s, &br, |after| {
        assert!(!after
            .side_one
            .volatile_statuses
            .contains(&PokemonVolatileStatus::ENCORE));
    });
}

/// Encore + Taunt on an encored status move that still has PP: no usable move, so the Pokemon Struggles
/// (Showdown: Taunt disables the encored status move, Encore disables the rest). Encore must persist, still
/// tied to the encored slot, so that when Taunt ends the Pokemon is locked into it again. The exp085
/// candidate recorded the Struggle as LastUsedMove::None, which left Encore without its move: the next turn
/// offered every move and resolving one panicked ("Encore should not be active when last used move is not
/// a move").
#[cfg(feature = "struggle")]
#[test]
fn test_encored_pokemon_that_struggles_stays_encored() {
    let mut s = state(8, 200);
    s.side_one
        .get_active()
        .replace_move(PokemonMoveIndex::M0, Choices::SWORDSDANCE);
    s.side_one.get_active().moves.m0.pp = 8;
    s.side_one
        .volatile_statuses
        .insert(PokemonVolatileStatus::ENCORE);
    s.side_one
        .volatile_statuses
        .insert(PokemonVolatileStatus::TAUNT);
    s.side_one.last_used_move = LastUsedMove::Move(PokemonMoveIndex::M0);
    assert_eq!(
        side_one_options(&s)
            .iter()
            .filter(|o| !o.starts_with("none") && !o.starts_with("pikachu"))
            .cloned()
            .collect::<Vec<_>>(),
        vec!["struggle".to_string()]
    );
    let br = turn(&mut s, "struggle", "splash");
    s.apply_instructions(&br[0].instruction_list);
    assert!(
        s.side_one
            .volatile_statuses
            .contains(&PokemonVolatileStatus::ENCORE),
        "Encore persists (Swords Dance has PP)"
    );
    assert!(
        matches!(
            s.side_one.last_used_move,
            LastUsedMove::Move(PokemonMoveIndex::M0)
        ),
        "{:?}",
        s.side_one.last_used_move
    );
    // Taunt wears off: locked into Swords Dance again, and resolving it works
    s.side_one
        .volatile_statuses
        .remove(&PokemonVolatileStatus::TAUNT);
    let o: Vec<String> = side_one_options(&s)
        .into_iter()
        .filter(|o| !o.starts_with("none"))
        .collect();
    assert_eq!(o, vec!["swordsdance".to_string()]);
    let _ = turn(&mut s, "swordsdance", "splash");
}

/// Control, unchanged by the fix (Showdown control_ordinary_encore): Encore on a move with PP left locks
/// the target for its next three turns (turns 2-4) and it is free on turn 5.
#[test]
fn test_ordinary_encore_length_matches_showdown() {
    let mut s = state(8, 200);
    let br = turn(&mut s, "dragonpulse", "encore");
    s.apply_instructions(&br[0].instruction_list);
    for t in 2..=4 {
        assert_eq!(
            side_one_options(&s),
            vec!["dragonpulse".to_string()],
            "turn {} locked",
            t
        );
        let br = turn(&mut s, "dragonpulse", "splash");
        s.apply_instructions(&br[0].instruction_list);
    }
    let o = side_one_options(&s);
    assert!(o.contains(&"icepunch".to_string()), "turn 5 free: {:?}", o);
}
