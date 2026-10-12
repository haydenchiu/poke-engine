#![cfg(not(any(feature = "gen1", feature = "gen2", feature = "gen3")))]
//! Pain Split sets both Pokemon to the average of their current HP, each capped at its OWN max HP.
//! Showdown (data/moves.ts painsplit + sim/pokemon.ts sethp):
//!     averagehp = Math.floor((targetHP + pokemon.hp) / 2) || 1;  target.sethp(averagehp); pokemon.sethp(averagehp)
//! and sethp clamps to the Pokemon's maxhp. Before the fix the engine gave both sides the uncapped average,
//! healing the side with the lower max HP past it (e.g. 160/155).

use poke_engine::choices::Choices;
use poke_engine::engine::generate_instructions::generate_instructions_from_move_pair;
use poke_engine::engine::state::{MoveChoice, PokemonVolatileStatus};
use poke_engine::instruction::{DamageInstruction, Instruction, StateInstructions};
use poke_engine::state::{PokemonMoveIndex, SideReference, State};

/// Side one uses Pain Split (moving first), side two Splash. Returns the single branch and the resulting
/// (user hp, target hp) after applying it.
fn pain_split(user: (i16, i16), target: (i16, i16)) -> (Vec<StateInstructions>, (i16, i16)) {
    let mut state = State::default();
    let u = state.side_one.get_active();
    u.replace_move(PokemonMoveIndex::M0, Choices::PAINSPLIT);
    (u.hp, u.maxhp) = user;
    u.speed = 200;
    let t = state.side_two.get_active();
    t.replace_move(PokemonMoveIndex::M0, Choices::SPLASH);
    (t.hp, t.maxhp) = target;
    t.speed = 100;
    let branches = generate_instructions_from_move_pair(
        &mut state,
        &MoveChoice::Move(PokemonMoveIndex::M0),
        &MoveChoice::Move(PokemonMoveIndex::M0),
        false,
    );
    assert_eq!(branches.len(), 1);
    state.apply_instructions(&branches[0].instruction_list);
    let hp = (
        state.side_one.get_active_immutable().hp,
        state.side_two.get_active_immutable().hp,
    );
    (branches, hp)
}

fn damage_instructions(branch: &StateInstructions) -> Vec<(SideReference, i16)> {
    branch
        .instruction_list
        .iter()
        .filter_map(|i| match i {
            Instruction::Damage(DamageInstruction {
                side_ref,
                damage_amount,
            }) => Some((*side_ref, *damage_amount)),
            _ => None,
        })
        .collect()
}

/// The T3 reproducer (sim/tests/engine_discrepancies.rs e1): both at full HP, user max 165, target max 155.
/// Average 160: the user drops to 160, the target stays at 155 (Showdown); the pin gave the target 160/155.
#[test]
fn test_painsplit_full_hp_target_capped_at_its_max() {
    let (branches, hp) = pain_split((165, 165), (155, 155));
    assert_eq!(hp, (160, 155));
    assert_eq!(
        damage_instructions(&branches[0]),
        vec![(SideReference::SideOne, 5), (SideReference::SideTwo, 0)]
    );
}

/// Unequal current HP, no cap involved: both move to the average (the upstream test_painsplit case).
#[test]
fn test_painsplit_unequal_hp_no_cap() {
    let (_, hp) = pain_split((50, 100), (60, 100));
    assert_eq!(hp, (55, 55));
}

/// Odd sum: the average is floored.
#[test]
fn test_painsplit_odd_sum_is_floored() {
    let (_, hp) = pain_split((51, 100), (60, 100));
    assert_eq!(hp, (55, 55));
}

/// The healed side is the TARGET and the average exceeds its max HP: target capped, user lowered.
#[test]
fn test_painsplit_target_capped_when_user_has_more_hp() {
    // average (300 + 50) / 2 = 175; target max 120
    let (branches, hp) = pain_split((300, 300), (50, 120));
    assert_eq!(hp, (175, 120));
    assert_eq!(
        damage_instructions(&branches[0]),
        vec![(SideReference::SideOne, 125), (SideReference::SideTwo, -70)]
    );
}

/// The healed side is the USER and the average exceeds its max HP: user capped, target lowered to the
/// uncapped average (the target's change does not depend on the user's cap).
#[test]
fn test_painsplit_user_capped_when_target_has_more_hp() {
    // average (40 + 300) / 2 = 170; user max 100
    let (branches, hp) = pain_split((40, 100), (300, 400));
    assert_eq!(hp, (100, 170));
    assert_eq!(
        damage_instructions(&branches[0]),
        vec![(SideReference::SideOne, -60), (SideReference::SideTwo, 130)]
    );
}

/// Exactly at the cap: the average equals the lower max HP, nothing is clamped.
#[test]
fn test_painsplit_average_equal_to_max_hp() {
    let (_, hp) = pain_split((165, 165), (145, 155));
    assert_eq!(hp, (155, 155));
}

/// Both at 1 HP: the average is 1 (Showdown's `|| 1` floor is unreachable in singles - two living Pokemon
/// always sum to at least 2 - but the result must still be 1, not 0).
#[test]
fn test_painsplit_both_at_one_hp() {
    let (_, hp) = pain_split((1, 100), (1, 100));
    assert_eq!(hp, (1, 1));
}

/// HP never leaves [0, max] on either side, over a grid of current/max HP.
#[test]
fn test_painsplit_never_exceeds_either_max_hp() {
    for user_max in [1i16, 7, 100, 165, 404] {
        for target_max in [1i16, 9, 100, 155, 714] {
            for user_hp in [1, user_max / 3 + 1, user_max] {
                for target_hp in [1, target_max / 2 + 1, target_max] {
                    let (_, (u, t)) = pain_split((user_hp, user_max), (target_hp, target_max));
                    let avg = std::cmp::max(1, (user_hp + target_hp) / 2);
                    assert_eq!(
                        (u, t),
                        (avg.min(user_max), avg.min(target_max)),
                        "user {}/{} target {}/{}",
                        user_hp,
                        user_max,
                        target_hp,
                        target_max
                    );
                }
            }
        }
    }
}

/// Unchanged by the fix: a Substitute blocks Pain Split entirely.
#[test]
fn test_painsplit_blocked_by_substitute_unchanged() {
    let mut state = State::default();
    state
        .side_one
        .get_active()
        .replace_move(PokemonMoveIndex::M0, Choices::PAINSPLIT);
    state
        .side_two
        .get_active()
        .replace_move(PokemonMoveIndex::M0, Choices::SPLASH);
    state.side_one.get_active().hp = 50;
    state.side_two.get_active().hp = 60;
    state
        .side_two
        .volatile_statuses
        .insert(PokemonVolatileStatus::SUBSTITUTE);
    state.side_two.substitute_health = 10;
    let branches = generate_instructions_from_move_pair(
        &mut state,
        &MoveChoice::Move(PokemonMoveIndex::M0),
        &MoveChoice::Move(PokemonMoveIndex::M0),
        false,
    );
    assert!(damage_instructions(&branches[0]).is_empty());
}
