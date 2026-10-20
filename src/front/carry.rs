//! Place carried stacks against the animated palms, after animation and
//! before transform propagation. Bounds include the item's display pose.

use bevy::app::AnimationSystems;
use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::prelude::*;
use bevy::transform::{helper::TransformHelper, TransformSystems};

#[derive(Clone, Copy)]
pub struct Grip {
    pub bone: Entity,
    pub at: Vec3,
}

#[derive(Component)]
pub struct CarryAnchor {
    pub parent: Entity,
    pub palms: [Grip; 2],
}

impl CarryAnchor {
    pub fn rig(parent: Entity, left: Entity, right: Entity) -> Self {
        // The hand bone starts at the wrist; fingers extend along its local +Y.
        Self { parent, palms: [left, right].map(|bone| Grip { bone, at: Vec3::Y * 0.06 }) }
    }

    pub fn figure(parent: Entity, arms: [Entity; 2]) -> Self {
        // Left/right in character space. The fallback arms hang down local -Y.
        Self { parent, palms: [arms[1], arms[0]].map(|bone| Grip { bone, at: Vec3::NEG_Y * 0.32 }) }
    }
}

pub struct CarryPlugin;

impl Plugin for CarryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, align_carried_items.after(AnimationSystems).before(TransformSystems::Propagate));
    }
}

/// The two side grips meet the palms, with the item's centre between them.
fn carry_pose(left: Vec3, right: Vec3, min: Vec3, max: Vec3) -> Option<Transform> {
    if !left.is_finite() || !right.is_finite() || !min.is_finite() || !max.is_finite() {
        return None;
    }
    let across = left - right;
    let width = max.x - min.x;
    if across.length() < 0.02 || width < 1e-4 {
        return None;
    }
    let x = across.normalize();
    let forward = Vec3::Z - x * x.dot(Vec3::Z);
    if forward.length_squared() < 1e-4 {
        return None;
    }
    let z = forward.normalize();
    let y = z.cross(x);
    let turn = Quat::from_mat3(&Mat3::from_cols(x, y, z));
    let scale = (across.length() / width).clamp(0.2, 0.85);
    let centre = (min + max) / 2.0;
    Some(Transform {
        translation: (left + right) / 2.0 - turn * (centre * scale),
        rotation: turn,
        scale: Vec3::splat(scale),
    })
}

#[allow(clippy::type_complexity)]
fn align_carried_items(
    anchors: Query<(Entity, &CarryAnchor, &Visibility)>,
    children: Query<&Children>,
    geometry: Query<(Option<&Mesh3d>, Option<&Aabb>, Option<&Visibility>)>,
    meshes: Res<Assets<Mesh>>,
    mut transforms: ParamSet<(TransformHelper, Query<&Transform>, Query<&mut Transform>)>,
) {
    // Read the current local bone transforms, not last frame's GlobalTransforms.
    let targets: Vec<_> = {
        let helper = transforms.p0();
        anchors.iter().filter(|(_, _, vis)| **vis != Visibility::Hidden).filter_map(|(entity, anchor, _)| {
            let parent = helper.compute_global_transform(anchor.parent).ok()?.affine().inverse();
            let [left, right] = anchor.palms.map(|grip| {
                helper.compute_global_transform(grip.bone).ok()
                    .map(|bone| parent.transform_point3(bone.transform_point(grip.at)))
            });
            Some((entity, left?, right?))
        }).collect()
    };
    let poses: Vec<_> = {
        let locals = transforms.p1();
        targets.into_iter().filter_map(|(entity, left, right)| {
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            let mut stack: Vec<_> = children.get(entity).ok()?.iter().map(|child| (child, Mat4::IDENTITY)).collect();
            while let Some((node, parent)) = stack.pop() {
                let Ok((mesh, aabb, visibility)) = geometry.get(node) else { continue };
                // Once a model loads its shape fallback is hidden. Counting both
                // would make the grip depend on the obsolete placeholder's size.
                if visibility == Some(&Visibility::Hidden) {
                    continue;
                }
                let Ok(local) = locals.get(node) else { continue };
                let pose = parent * local.to_matrix();
                if let Some(bounds) = aabb.copied().or_else(|| mesh.and_then(|m| meshes.get(&m.0)).and_then(MeshAabb::get_aabb)) {
                    let centre = Vec3::from(bounds.center);
                    let half = Vec3::from(bounds.half_extents);
                    for x in [-1.0, 1.0] {
                        for y in [-1.0, 1.0] {
                            for z in [-1.0, 1.0] {
                                let point = pose.transform_point3(centre + half * Vec3::new(x, y, z));
                                min = min.min(point);
                                max = max.max(point);
                            }
                        }
                    }
                }
                if let Ok(descendants) = children.get(node) {
                    stack.extend(descendants.iter().map(|child| (child, pose)));
                }
            }
            if !min.is_finite() || !max.is_finite() {
                return None;
            }
            Some((entity, carry_pose(left, right, min, max)?))
        }).collect()
    };
    let mut locals = transforms.p2();
    for (entity, pose) in poses {
        if let Ok(mut transform) = locals.get_mut(entity) {
            *transform = pose;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::transform::TransformPlugin;

    #[test]
    fn offset_and_tilted_items_touch_both_palms() {
        let min = Vec3::new(-0.08, 0.005, -0.19);
        let max = Vec3::new(0.32, 0.27, 0.21);
        let left = Vec3::new(0.15, 0.81, 0.30);
        let right = Vec3::new(-0.15, 0.78, 0.30);
        let pose = carry_pose(left, right, min, max).unwrap();
        let centre = (min + max) / 2.0;
        assert!((pose.transform_point(Vec3::new(max.x, centre.y, centre.z)) - left).length() < 1e-5);
        assert!((pose.transform_point(Vec3::new(min.x, centre.y, centre.z)) - right).length() < 1e-5);
        assert!((pose.transform_point(centre) - (left + right) / 2.0).length() < 1e-5);
    }

    #[test]
    fn missing_geometry_or_degenerate_grips_do_not_produce_invalid_poses() {
        assert!(carry_pose(Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ONE).is_none());
        assert!(carry_pose(Vec3::X, Vec3::NEG_X, Vec3::ZERO, Vec3::ZERO).is_none());
        assert!(carry_pose(Vec3::splat(f32::NAN), Vec3::ZERO, Vec3::ZERO, Vec3::ONE).is_none());
        let pose = carry_pose(Vec3::X * 10.0, Vec3::NEG_X * 10.0, Vec3::ZERO, Vec3::ONE).unwrap();
        assert_eq!(pose.scale, Vec3::splat(0.85), "transitions cannot grow a stack without limit");
    }

    #[derive(Component)]
    struct TestHand;

    #[test]
    fn carried_mesh_tracks_this_frames_hands_under_a_scaled_rotated_rig() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, CarryPlugin))
            .init_resource::<Assets<Mesh>>();
        // Stand in for the animation evaluator, including a change after Update.
        app.add_systems(PostUpdate, (|mut hands: Query<&mut Transform, With<TestHand>>| {
            for mut hand in &mut hands {
                hand.translation += Vec3::new(0.0, 0.03, 0.02);
            }
        }).in_set(AnimationSystems));
        let root = app.world_mut().spawn((Transform::from_xyz(12.0, 0.8, -9.0)
            .with_rotation(Quat::from_rotation_y(1.2)), Visibility::default())).id();
        let rig = app.world_mut().spawn((Transform::from_xyz(0.0, 0.07, 0.03)
            .with_scale(Vec3::splat(0.55)), ChildOf(root))).id();
        let left = app.world_mut().spawn((Transform::from_xyz(0.2, 1.3, 0.38)
            .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)), TestHand, ChildOf(rig))).id();
        let right = app.world_mut().spawn((Transform::from_xyz(-0.2, 1.3, 0.38)
            .with_rotation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2)), TestHand, ChildOf(rig))).id();
        let carrier = app.world_mut().spawn((Transform::from_xyz(0.0, -8.0, 0.0),
            Visibility::default(), CarryAnchor::rig(root, left, right), ChildOf(root))).id();
        let display = app.world_mut().spawn((Transform::from_xyz(0.15, 0.07, -0.09)
            .with_scale(Vec3::splat(0.7)).with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
            Visibility::default(), ChildOf(carrier))).id();
        let item = app.world_mut().spawn((Transform::default(), Visibility::default(),
            Mesh3d::default(), Aabb::from_min_max(Vec3::new(-0.16, 0.0, -0.24), Vec3::new(0.16, 0.22, 0.24)), ChildOf(display))).id();
        app.world_mut().spawn((Transform::default(), Visibility::Hidden, Mesh3d::default(),
            Aabb::from_min_max(Vec3::splat(-99.0), Vec3::splat(99.0)), ChildOf(carrier)));
        for frame in 0..4 {
            if frame == 2 {
                let mut parent = app.world_mut().get_mut::<Transform>(root).unwrap();
                parent.translation += Vec3::new(-3.0, 0.5, 4.0);
                parent.rotation = Quat::from_rotation_y(-0.9);
            }
            app.update();
            let world = app.world();
            let mesh = world.get::<GlobalTransform>(item).unwrap();
            let left_palm = world.get::<GlobalTransform>(left).unwrap().transform_point(Vec3::Y * 0.06);
            let right_palm = world.get::<GlobalTransform>(right).unwrap().transform_point(Vec3::Y * 0.06);
            // The display turns native +Z to +X, so these are the two side contacts.
            assert!((mesh.transform_point(Vec3::new(0.0, 0.11, 0.24)) - left_palm).length() < 1e-4, "left palm, frame {frame}");
            assert!((mesh.transform_point(Vec3::new(0.0, 0.11, -0.24)) - right_palm).length() < 1e-4, "right palm, frame {frame}");
        }
    }
}
