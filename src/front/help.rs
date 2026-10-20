//! The handbook (H): help by topic, in the style of a field manual. A list of
//! topics on the left (with a search over all their text), the page on the right.

use super::input::UiState;
use super::theme::{self, FieldWindow};
use bevy_egui::egui::{self, Align2, RichText};
use my_world::defs::{BuildId, Special};

/// One piece of a page.
enum Block {
    /// A paragraph.
    Text(&'static str),
    /// A point in a list.
    Point(&'static str),
    /// Keys and what they do.
    Keys(&'static [(&'static str, &'static str)]),
    /// A boxed tip worth remembering.
    Note(&'static str),
    /// Every buildable with its shortcut and cost (from the definitions).
    Buildables,
}

use Block::*;

struct Section {
    heading: &'static str,
    blocks: &'static [Block],
}

pub struct Topic {
    pub id: &'static str,
    pub title: &'static str,
    /// One line under the title.
    summary: &'static str,
    sections: &'static [Section],
}

pub const TOPICS: &[Topic] = &[
    Topic {
        id: "basics",
        title: "Getting started",
        summary: "You don't control colonists directly: you give orders, and they pick up the work.",
        sections: &[
            Section {
                heading: "Orders, not commands",
                blocks: &[
                    Text("Mark what should be done (trees to chop, rock to mine, things to build) and colonists take the work on according to their work priorities."),
                    Point("Work (P) sets who does what: left click raises a priority, right click lowers it."),
                    Point("With a colonist selected, right-click a cell for what they could do there: they do it now, even work they're not assigned to."),
                    Point("F drafts the selected colonist (or everyone); drafted, right click moves them or attacks."),
                ],
            },
            Section {
                heading: "Finding things out",
                blocks: &[
                    Point("Esc is the Select tool: click colonists or cells to inspect them."),
                    Point("Info (top bar) looks up any item, building or plant; Info buttons in the inspector open it too."),
                    Point("Alerts in the top right say what needs attention."),
                    Note("Raids come every few days. Build walls and gear up before the first one arrives."),
                ],
            },
            Section {
                heading: "Time",
                blocks: &[
                    Keys(&[("Space", "Pause and resume"), ("1-4", "Speed 1x, 2x, 5x, 10x")]),
                    Text("A year has four seasons of 5 days. Crops grow in spring and summer, slower in autumn and not at all in winter; winter's first frost kills those not yet ripe. Sow in time, and stock up for winter."),
                ],
            },
        ],
    },
    Topic {
        id: "orders",
        title: "Orders and zones",
        summary: "Designate work and paint areas with the build menu on the left.",
        sections: &[
            Section {
                heading: "Orders",
                blocks: &[
                    Text("Mark trees to chop, rock and soil to mine and berry bushes to forage. Cancel takes designations and blueprints away."),
                    Keys(&[("Backspace", "Deconstruct"), ("Left drag", "Paint the tool over a rectangle"), ("Right click", "Back to Select")]),
                ],
            },
            Section {
                heading: "Zones",
                blocks: &[
                    Point("Stockpile zones: items get hauled there. Click one to set its priority and which items it takes."),
                    Point("Grow zones plant a crop; click one to pick it."),
                    Point("Quarries are dug out level by level, with a ladder down at one edge; click one to set its depth."),
                    Point("Safe zones are where civilians shelter in a raid."),
                    Point("Clear zone removes zones again."),
                ],
            },
        ],
    },
    Topic {
        id: "building",
        title: "Building",
        summary: "Blueprints, materials and what can be built.",
        sections: &[
            Section {
                heading: "Blueprints",
                blocks: &[
                    Text("Colonists haul the materials to blueprints, then build them. The build menu lists everything, grouped and searchable, with costs (red if the colony is short); hover an entry for details, right-click for its full info."),
                    Keys(&[
                        ("`", "Hide or show the build menu"),
                        (", / .", "Turn furniture or stairs while placing or moving"),
                        ("Shift", "With walls: fill the rectangle, not just its outline"),
                    ]),
                    Point("The arrow on furniture points to its front, where it's used from."),
                    Point("Click a wall with a door or window to build it in; the wall stands until it's done."),
                    Point("Click furniture and press Move to have a builder carry it somewhere else."),
                ],
            },
            Section { heading: "Buildables", blocks: &[Buildables] },
        ],
    },
    Topic {
        id: "storeys",
        title: "Storeys and roofs",
        summary: "Floors, stairs, shafts and the layer view.",
        sections: &[
            Section {
                heading: "Building up",
                blocks: &[
                    Point("Floors are for upper storeys; they need a wall, door, stairs or ground below within 3 cells."),
                    Point("Walls stack onto walls and doors below, even planned ones, to build several storeys high."),
                    Point("Stairs rise away from where you walk onto them, onto the level above beside them; ladders go straight up."),
                    Keys(&[("PgDn / PgUp", "Layer view: hide everything above a level, and build on it"), ("[ / ]", "The same"), ("Shift+R", "Hide roofs, to look into buildings")]),
                ],
            },
            Section {
                heading: "Roofs",
                blocks: &[
                    Text("Enclose an area with walls and doors and it gets a roof by itself: builders put it up with wood. A roof needs a wall within 3 cells, like floors; red crosses mark where it can't reach."),
                ],
            },
            Section {
                heading: "Digging down",
                blocks: &[
                    Point("Mining also digs soil. Dig straight down to sink a shaft; put stairs in it."),
                    Note("Anything left with nothing underneath falls - more than one level hurts."),
                ],
            },
        ],
    },
    Topic {
        id: "production",
        title: "Production and storage",
        summary: "Workbenches, bills, materials and power.",
        sections: &[
            Section {
                heading: "Bills",
                blocks: &[
                    Text("Click a workbench to add bills: make N, make until you have N, or forever (Shift+click on + or - steps by 5). Haulers bring the ingredients; cooks and crafters make it."),
                    Point("Benches: stove, sawing bench, stonecutter, smelter, smithy, crafting bench, then machinist's bench, refinery, chemistry lab, electronics bench and fabricator - each built from what the ones before make."),
                    Point("Mine iron ore deep in the rock and smelt it into steel; make weapons and armour at a smithy."),
                    Point("Gear needs components (mechanical, electrical, optical, structural, chemical, textile), made at the fitting benches."),
                ],
            },
            Section {
                heading: "Storage",
                blocks: &[
                    Point("Stockpiles have a priority and a list of what they take; things move to better stockpiles."),
                    Point("Shelves are storage by themselves and hold four stacks; click one to set what goes on it (shelves side by side share settings)."),
                ],
            },
            Section {
                heading: "Power",
                blocks: &[
                    Text("One grid for the whole colony, no cables. PV systems (open sky only) make up to 1 kW in daylight, home batteries store 10 kWh. Electric heaters and coolers keep their room's temperature while there's power. The Resources window shows the grid."),
                ],
            },
        ],
    },
    Topic {
        id: "rooms",
        title: "Rooms and comfort",
        summary: "Room quality, themes, light and furniture.",
        sections: &[
            Section {
                heading: "Room quality",
                blocks: &[
                    Text("A room needs walls and a roof. Its quality (size, beauty of furniture, windows and doors, cleanliness, light) affects mood when colonists sleep or eat there. Click a cell inside a room to see why it's good or bad."),
                    Point("Lamps light up rooms and caves - nights are dark."),
                    Point("Colonists eat at a table if there's a chair beside it, and hate eating without."),
                    Point("Sleeping in a roofed bedroom makes colonists happier."),
                ],
            },
            Section {
                heading: "Themed rooms",
                blocks: &[
                    Text("Furnish a room with what a theme needs - bedroom, barracks, kitchen, hospital, surgery, smithy, workshop - and its floor and name show it. The Rooms button lists what each needs; click a cell in a room for a checklist."),
                ],
            },
            Section {
                heading: "Wall decor",
                blocks: &[Text("Point at a wall and click to hang it: five places on each side of a wall.")],
            },
        ],
    },
    Topic {
        id: "needs",
        title: "Colonists' needs",
        summary: "Food, rest, fun, warmth and the daily schedule.",
        sections: &[
            Section {
                heading: "Food",
                blocks: &[
                    Text("Raw food can cause food poisoning - cook meals at a stove. Cooked meals are better and fill more."),
                ],
            },
            Section {
                heading: "Fun and schedules",
                blocks: &[
                    Text("Colonists need fun: chess tables, bookshelves, dartboards, strolls. Their day is painted in Work > Schedule: sleep, fun, work or anything."),
                ],
            },
            Section {
                heading: "Warmth",
                blocks: &[
                    Text("Rooms keep warmth and are warmed by what burns in them. Below 0°C colonists freeze and hypothermia builds up; clothes keep them warm. The cell inspector shows a room's temperature."),
                    Note("Colonists put on a parka and hat by themselves when it gets cold, unless their loadout says otherwise."),
                ],
            },
        ],
    },
    Topic {
        id: "health",
        title: "Health and medicine",
        summary: "Wounds, doctors, burials.",
        sections: &[
            Section {
                heading: "Wounds",
                blocks: &[
                    Text("Wounds bleed and can get infected. Doctors (see P) rescue downed colonists to a bed and tend them - better with medicine and skill. Click a colonist, then Health for details."),
                ],
            },
            Section {
                heading: "The dead",
                blocks: &[Text("Dig graves (Structure menu, on soil) for the dead: haulers bury them. Unburied dead upset everyone.")],
            },
        ],
    },
    Topic {
        id: "gear",
        title: "Gear and loadouts",
        summary: "Weapons, armour and what each colonist keeps on.",
        sections: &[
            Section {
                heading: "Loadouts",
                blocks: &[
                    Text("Colonists pick up better gear. Select one and right-click a weapon or clothes to put it in their loadout (Combat tab): they fetch and keep it."),
                    Point("Slots: weapon, sidearm, utility belt, head, face, shirt, jacket, legs and back."),
                    Point("Armour protects only where it's worn; the Info window says what each piece covers."),
                ],
            },
        ],
    },
    Topic {
        id: "battle",
        title: "Raids and battle",
        summary: "Paying off raiders, or fighting them turn by turn.",
        sections: &[
            Section {
                heading: "Raids",
                blocks: &[Text("Raids come every few days. Pay what the raiders demand, or fight a turn-based battle (the colony stops).")],
            },
            Section {
                heading: "In battle",
                blocks: &[
                    Keys(&[("Click unit / Tab", "Pick a unit"), ("Click cell", "Move there"), ("Click enemy", "Attack"), ("Space", "End the unit's turn")]),
                    Text("Over each unit: health per member, then morale and suppression (the tick is where it breaks or gets pinned). Hover the bars in the battle panel to see what they do."),
                    Note("Mark civilians and paint a safe zone: they shelter there instead of fighting."),
                ],
            },
        ],
    },
    Topic {
        id: "squads",
        title: "Squads and expeditions",
        summary: "Leaders, members and missions away from the colony.",
        sections: &[
            Section {
                heading: "Squads",
                blocks: &[
                    Text("In a colonist's Combat tab make them a leader, put others in their squad and set loadouts. Far-away squad members join their leader a few rounds in."),
                    Point("A squad is all ranged or all melee (by the loadout weapon, or the one they carry)."),
                    Point("Leaders want a decent bedroom of their own: click a bed to choose who sleeps in it."),
                ],
            },
            Section {
                heading: "Expeditions",
                blocks: &[
                    Text("Pick a mission on offer, choose who goes and what they pack. They leave the map for days; decide what they do when something comes up. Fights are played on a map of their own."),
                ],
            },
        ],
    },
    Topic {
        id: "controls",
        title: "Controls",
        summary: "Every key at a glance.",
        sections: &[
            Section {
                heading: "Camera",
                blocks: &[Keys(&[
                    ("WASD / arrows", "Pan"),
                    ("Middle drag", "Pan"),
                    ("Q / E", "Rotate"),
                    ("Right drag", "Orbit and tilt"),
                    ("Wheel", "Zoom"),
                    ("C", "Center on the selection"),
                    ("Double click card", "Select a colonist and center them"),
                ])],
            },
            Section {
                heading: "Windows",
                blocks: &[Keys(&[
                    ("P", "Work priorities"),
                    ("I", "Resources"),
                    ("H", "This handbook"),
                    ("Esc", "Menu (save, load, new game, sound); closes windows"),
                ])],
            },
            Section {
                heading: "Game",
                blocks: &[Keys(&[
                    ("Space", "Pause"),
                    ("1-4", "Speed 1x, 2x, 5x, 10x"),
                    ("F", "Draft"),
                    ("K / L", "Save / load"),
                    ("Shift+N", "New colony"),
                ])],
            },
        ],
    },
];

pub fn topic_index(id: &str) -> usize {
    TOPICS.iter().position(|t| t.id == id).unwrap_or(0)
}

/// One line per buildable the player can place: shortcut, name and cost.
fn buildables() -> Vec<(String, String, String)> {
    BuildId::all()
        .filter(|kind| kind.def().special != Special::Roof)
        .map(|kind| {
            let def = kind.def();
            (def.key.as_deref().unwrap_or("").to_uppercase(), def.name.clone(), def.cost_text())
        })
        .collect()
}

/// All of a topic's words, lowercased, for the search.
fn text_of(t: &Topic) -> String {
    let mut s = format!("{} {} ", t.title, t.summary);
    for sec in t.sections {
        s.push_str(sec.heading);
        s.push(' ');
        for b in sec.blocks {
            match b {
                Text(x) | Point(x) | Note(x) => s.push_str(x),
                Keys(keys) => keys.iter().for_each(|(k, d)| {
                    s.push_str(k);
                    s.push(' ');
                    s.push_str(d);
                }),
                Buildables => buildables().iter().for_each(|(k, n, c)| s.push_str(&format!("{} {} {} ", k, n, c))),
            }
            s.push(' ');
        }
    }
    s.to_lowercase()
}

const LIST_W: f32 = 178.0;
const PAGE_W: f32 = 520.0;
const PAGE_H: f32 = 440.0;

pub fn window(ctx: &egui::Context, st: &mut UiState) {
    if !st.show_help {
        return;
    }
    let mut open = true;
    let mut topic = st.help_topic.min(TOPICS.len() - 1);
    let filter = &mut st.help_filter;
    FieldWindow::new("Field handbook")
        .id(egui::Id::new("help"))
        .open(&mut open)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(LIST_W);
                    ui.add(egui::TextEdit::singleline(filter).hint_text("Search the handbook").desired_width(LIST_W));
                    ui.add_space(4.0);
                    let query = filter.trim().to_lowercase();
                    let mut shown = 0;
                    for (k, t) in TOPICS.iter().enumerate() {
                        if !query.is_empty() && !text_of(t).contains(&query) {
                            continue;
                        }
                        shown += 1;
                        if theme::list_entry(ui, topic == k, &format!("{:02}", k + 1), t.title, LIST_W).clicked() {
                            topic = k;
                        }
                    }
                    if shown == 0 {
                        ui.label(RichText::new("Nothing matches").color(theme::CREAM_DIM));
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("H or Esc closes the handbook.").small().color(theme::CREAM_DIM));
                });
                ui.add_space(4.0);
                let (rule, _) = ui.allocate_exact_size(egui::vec2(1.0, PAGE_H), egui::Sense::hover());
                ui.painter().rect_filled(rule, 0.0, theme::OLIVE_DIM);
                ui.add_space(8.0);
                ui.vertical(|ui| {
                    ui.set_width(PAGE_W);
                    egui::ScrollArea::vertical().id_salt(("help_page", topic)).max_height(PAGE_H).auto_shrink([false, false]).show(ui, |ui| {
                        page(ui, &TOPICS[topic]);
                    });
                });
            });
        });
    st.help_topic = topic;
    if !open {
        st.show_help = false;
    }
}

fn page(ui: &mut egui::Ui, t: &Topic) {
    ui.set_width(PAGE_W - 14.0);
    ui.set_max_width(PAGE_W - 14.0);
    ui.label(theme::caps(t.title, 26.0).color(theme::CREAM_BRIGHT));
    ui.label(RichText::new(t.summary).italics().color(theme::CREAM_DIM));
    ui.add_space(6.0);
    for sec in t.sections {
        ui.add_space(4.0);
        theme::section(ui, sec.heading);
        for b in sec.blocks {
            block(ui, b);
        }
    }
}

fn block(ui: &mut egui::Ui, b: &Block) {
    match b {
        Text(x) => {
            ui.add(egui::Label::new(*x).wrap());
        }
        Point(x) => theme::bullet(ui, |ui| {
            ui.add(egui::Label::new(*x).wrap());
        }),
        Note(x) => {
            theme::Plate::inset().stroke(egui::Stroke::new(1.0, theme::TEAL)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.label(theme::caps("Note", 14.0).color(theme::TEAL));
                    ui.add(egui::Label::new(RichText::new(*x).color(theme::CREAM)).wrap());
                });
            });
            ui.add_space(2.0);
        }
        Keys(keys) => {
            egui::Grid::new(ui.next_auto_id()).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                for (k, d) in keys.iter() {
                    ui.horizontal(|ui| {
                        for (n, part) in k.split(" / ").enumerate() {
                            if n > 0 {
                                ui.label(RichText::new("/").color(theme::CREAM_DIM));
                            }
                            theme::key_cap(ui, part);
                        }
                    });
                    ui.add(egui::Label::new(*d).wrap());
                    ui.end_row();
                }
            });
        }
        Buildables => {
            egui::Grid::new("help_buildables").num_columns(3).striped(true).spacing([12.0, 2.0]).max_col_width(260.0).show(ui, |ui| {
                ui.label(theme::caps("Key", 13.0).color(theme::CREAM_DIM));
                ui.label(theme::caps("Building", 13.0).color(theme::CREAM_DIM));
                ui.label(theme::caps("Cost", 13.0).color(theme::CREAM_DIM));
                ui.end_row();
                for (key, name, cost) in buildables() {
                    if key.is_empty() {
                        ui.label("");
                    } else {
                        theme::key_cap(ui, &key);
                    }
                    ui.label(RichText::new(name).color(theme::CREAM));
                    ui.add(egui::Label::new(RichText::new(cost).small().color(theme::CREAM_DIM)).wrap());
                    ui.end_row();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topics_have_unique_ids_and_are_searchable() {
        for (k, t) in TOPICS.iter().enumerate() {
            assert_eq!(topic_index(t.id), k, "{} is listed twice", t.id);
            assert!(!t.sections.is_empty(), "{} has no sections", t.id);
        }
        assert!(text_of(&TOPICS[topic_index("building")]).contains("wall"), "the buildables are searched too");
        assert!(text_of(&TOPICS[topic_index("controls")]).contains("pause"));
    }
}
