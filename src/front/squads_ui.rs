//! Setting up squads during normal play: the Combat tab of the colonist
//! inspector (role, squad, loadout) and the Squads overview window.

use super::input::{Selection, UiState};
use super::sim::Sim;
use bevy_egui::egui::{self, Align2, Color32, RichText};
use my_world::defs::{ItemId, Slot};
use my_world::map::Zone;
use my_world::pawn::{Pawn, Role};
use my_world::battle::CELLS_PER_ROUND;
use my_world::squads::{fights_at_range, style_name, SQUAD_MAX};
use my_world::world::World as SimWorld;


/// A change asked for in the Combat tab, applied after drawing.
pub enum CombatAction {
    Role(Role),
    Loadout(Vec<ItemId>),
}

fn label_or_none(kind: Option<ItemId>) -> String {
    kind.map_or("None".to_string(), |k| k.name().to_string())
}

/// One loadout slot: a dropdown of every item that fits it, with how many are in storage.
fn slot_choice(
    ui: &mut egui::Ui,
    w: &SimWorld,
    id: u32,
    label: &str,
    current: Option<ItemId>,
    fits: impl Fn(ItemId) -> bool,
) -> Option<Option<ItemId>> {
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.add_sized([70.0, 18.0], egui::Label::new(label));
        egui::ComboBox::from_id_salt(("loadout", id, label)).selected_text(label_or_none(current)).show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "None").clicked() {
                chosen = Some(None);
            }
            for k in ItemId::all().filter(|&k| fits(k)) {
                let text = format!("{} ({} stored)", k.name(), w.map.count_items(k));
                if ui.selectable_label(current == Some(k), text).clicked() {
                    chosen = Some(Some(k));
                }
            }
        });
    });
    chosen
}

/// How well a colonist fights: their skills, and the chances to hit with the
/// weapon they have (before cover and the like).
fn fighting(ui: &mut egui::Ui, p: &Pawn) {
    use my_world::battle::base_hit_chance;
    use my_world::pawn::Skill;
    ui.label(RichText::new("Fighting").strong());
    ui.label(format!("Shooting {}   Melee {}", p.skill(Skill::Shooting), p.skill(Skill::Melee)));
    let w = p.attack();
    let pct = |c: f32| format!("{:.0}%", c.clamp(0.05, 0.95) * 100.0);
    let text = if w.ranged {
        let far = w.range.floor().max(1.0);
        format!(
            "Hits with {}: {} at 3 cells, {} at {:.0} (its range), before cover",
            p.weapon_name().to_lowercase(),
            pct(base_hit_chance(p, true, 3.0)),
            pct(base_hit_chance(p, true, far)),
            far
        )
    } else {
        format!("Hits with {}: {} of blows", p.weapon_name().to_lowercase(), pct(base_hit_chance(p, false, 1.0)))
    };
    ui.label(text);
    let how = "Shots: 55% + 2.5% per Shooting level - 2.5% per cell away; blows: 60% + 2% per Melee level. \
               Hurt eyes or hands lower both; in battle, cover, hunkering down, height and being pinned change it.";
    ui.label(RichText::new(how).small().weak());
}

/// Role, squad and loadout of a colonist.
pub fn combat_tab(ui: &mut egui::Ui, w: &SimWorld, p: &Pawn) -> Vec<CombatAction> {
    let mut actions = Vec::new();
    fighting(ui, p);
    ui.separator();
    ui.label(RichText::new("When a battle starts").strong());
    let mut role = p.role;
    ui.horizontal(|ui| {
        ui.radio_value(&mut role, Role::Fighter, "Fighter").on_hover_text("Fights as a unit of their own");
        ui.radio_value(&mut role, Role::Civilian, "Civilian").on_hover_text("Shelters in the safe zone");
        ui.radio_value(&mut role, Role::Leader, "Squad leader")
            .on_hover_text("Others can join their squad; needs a decent bedroom of their own");
    });
    let leaders: Vec<&Pawn> = w.pawns.iter().filter(|o| o.role == Role::Leader && o.id != p.id && !o.dead).collect();
    if !leaders.is_empty() {
        ui.horizontal(|ui| {
            let current = match p.role {
                Role::Member(l) => w.pawn(l).map_or("?".to_string(), |l| format!("{}'s squad", l.name)),
                _ => "No squad".to_string(),
            };
            ui.label("Squad:");
            egui::ComboBox::from_id_salt(("squad", p.id)).selected_text(current).show_ui(ui, |ui| {
                for l in leaders {
                    let size = 1 + w.squad_members(l.id).len();
                    let style = style_name(fights_at_range(l));
                    let text = format!("{}'s {} squad ({}/{})", l.name, style, size, SQUAD_MAX);
                    // Only squads that fight the way they do.
                    let fits = fights_at_range(l) == fights_at_range(p);
                    let item = ui.add_enabled(fits, egui::Button::selectable(p.role == Role::Member(l.id), text));
                    let item = item.on_disabled_hover_text(format!(
                        "{} fights {}: give them a {} weapon in the loadout first",
                        p.name,
                        style_name(fights_at_range(p)),
                        style
                    ));
                    if item.clicked() {
                        role = Role::Member(l.id);
                    }
                }
            });
        });
    }
    if role != p.role {
        actions.push(CombatAction::Role(role));
    }
    if p.role == Role::Leader {
        let members: Vec<String> =
            w.squad_members(p.id).iter().filter_map(|&m| w.pawn(m).map(|m| m.name.clone())).collect();
        let text = if members.is_empty() { "No one in the squad yet".to_string() } else { members.join(", ") };
        ui.label(RichText::new(format!("{} squad: {}", capitalised(style_name(fights_at_range(p))), text)).weak());
        if p.poor_quarters {
            let text = "Wants a decent bedroom of their own. Click a bed to give it to them.";
            ui.label(RichText::new(text).color(Color32::from_rgb(255, 140, 100)));
        }
    }

    ui.separator();
    ui.label(RichText::new("Loadout").strong());
    let about = "The gear they use: fetched from storage and kept on (right-click gear to set it too). Empty slots: they take the best they find.";
    ui.label(RichText::new(about).small().weak());
    let style = style_name(fights_at_range(p));
    let rule = match p.role {
        Role::Member(_) => format!("Fights {}, like the whole squad: the weapon must stay {}.", style, style),
        Role::Leader => format!("Fights {}. A weapon of the other kind makes it a squad of that kind.", style),
        _ => format!("Fights {}.", style),
    };
    ui.label(RichText::new(rule).small().weak());
    let weapon = p.loadout.iter().copied().find(|k| k.def().weapon.is_some());
    let worn = |slot: Slot| p.loadout.iter().copied().find(|k| k.def().apparel.as_ref().is_some_and(|a| a.slot == slot));
    let mut change: Option<(Option<Slot>, Option<ItemId>)> = None;
    if let Some(c) = slot_choice(ui, w, p.id, "Weapon", weapon, |k| k.def().weapon.is_some()) {
        change = Some((None, c));
    }
    for (slot, label) in [(Slot::Body, "Clothes"), (Slot::Outer, "Armour"), (Slot::Head, "Helmet")] {
        let fits = |k: ItemId| k.def().apparel.as_ref().is_some_and(|a| a.slot == slot);
        if let Some(c) = slot_choice(ui, w, p.id, label, worn(slot), fits) {
            change = Some((Some(slot), c));
        }
    }
    if let Some((slot, item)) = change {
        let same_slot = |k: &ItemId| match slot {
            None => k.def().weapon.is_some(),
            Some(s) => k.def().apparel.as_ref().is_some_and(|a| a.slot == s),
        };
        let mut loadout: Vec<ItemId> = p.loadout.iter().copied().filter(|k| !same_slot(k)).collect();
        loadout.extend(item);
        actions.push(CombatAction::Loadout(loadout));
    }
    ui.horizontal(|ui| {
        if ui.button("Use current gear").clicked() {
            actions.push(CombatAction::Loadout(p.weapon.into_iter().chain(p.apparel.iter().copied()).collect()));
        }
        if !p.loadout.is_empty() && ui.button("Clear").clicked() {
            actions.push(CombatAction::Loadout(Vec::new()));
        }
    });
    actions
}

pub fn apply(w: &mut SimWorld, id: u32, actions: Vec<CombatAction>) {
    for a in actions {
        match a {
            CombatAction::Role(role) => {
                if let Err(e) = w.set_role(id, role) {
                    w.push_log(format!("Can't do that: {}.", e));
                }
            }
            CombatAction::Loadout(items) => {
                if let Err(e) = w.set_loadout(id, &items) {
                    w.push_log(format!("Can't do that: {}.", e));
                }
            }
        }
    }
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

/// Roughly how many rounds a member needs to reach their leader.
fn rounds_away(a: &Pawn, b: &Pawn) -> usize {
    let cells = a.pos.0.abs_diff(b.pos.0) + a.pos.1.abs_diff(b.pos.1) + a.pos.2.abs_diff(b.pos.2);
    cells.saturating_sub(2).div_ceil(CELLS_PER_ROUND)
}

/// Overview of squads, fighters and civilians; clicking a name selects them.
pub fn squads_window(ctx: &egui::Context, sim: &Sim, st: &mut UiState) {
    let w = &sim.world;
    let colonists: Vec<&Pawn> = w.pawns.iter().filter(|p| p.is_living_colonist()).collect();
    let mut select = None;
    let mut open = st.show_squads;
    let name = |ui: &mut egui::Ui, p: &Pawn, extra: String, select: &mut Option<u32>| {
        if ui.link(format!("{}{}", p.name, extra)).clicked() {
            *select = Some(p.id);
        }
    };
    egui::Window::new("Squads")
        .open(&mut open)
        .anchor(Align2::RIGHT_TOP, [-8.0, 92.0])
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            let leaders: Vec<&&Pawn> = colonists.iter().filter(|p| p.role == Role::Leader).collect();
            if leaders.is_empty() {
                ui.label(RichText::new("No squads: everyone fights on their own.").weak());
            }
            for l in leaders {
                let members: Vec<&&Pawn> = colonists.iter().filter(|p| p.role == Role::Member(l.id)).collect();
                ui.horizontal(|ui| {
                    let style = style_name(fights_at_range(l));
                    name(ui, l, format!(" ({}, {}/{})", style, 1 + members.len(), SQUAD_MAX), &mut select);
                    if l.poor_quarters {
                        ui.label(RichText::new("wants a decent bedroom").small().color(Color32::from_rgb(255, 140, 100)));
                    }
                });
                for m in members {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        let away = match rounds_away(m, l) {
                            0 => " - with them".to_string(),
                            r => format!(" - about {} round{} away", r, if r == 1 { "" } else { "s" }),
                        };
                        name(ui, m, away, &mut select);
                        // Picked up a weapon of the other kind (and has none in the loadout).
                        if fights_at_range(m) != fights_at_range(l) {
                            let text = format!("armed {}: fights alone", style_name(fights_at_range(m)));
                            ui.label(RichText::new(text).small().color(Color32::from_rgb(255, 140, 100)));
                        }
                    });
                }
            }
            for (role, title) in [(Role::Fighter, "Fighting alone"), (Role::Civilian, "Civilians")] {
                let group: Vec<&&Pawn> = colonists.iter().filter(|p| p.role == role).collect();
                if group.is_empty() {
                    continue;
                }
                ui.separator();
                ui.label(RichText::new(title).strong());
                ui.horizontal_wrapped(|ui| {
                    for p in group {
                        name(ui, p, String::new(), &mut select);
                    }
                });
            }
            let civilians = colonists.iter().any(|p| p.role == Role::Civilian);
            if civilians && !w.map.zone.contains(&Zone::Safe) {
                ui.label(RichText::new("No safe zone: civilians stay where they are.").color(Color32::from_rgb(255, 140, 100)));
            }
            ui.separator();
            ui.label(RichText::new("Set roles and loadouts in a colonist's Combat tab.").small().weak());
        });
    st.show_squads = open;
    if let Some(id) = select {
        st.select(Some(Selection::Pawn(id)));
        st.inspector_tab = super::input::InspectorTab::Combat;
    }
}
