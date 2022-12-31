use bevy_core_pipeline::{core_3d, prelude::Camera3d};
use bevy_ecs::{
    prelude::{Bundle, Component, ReflectComponent, Without},
    system::{Query, Res, ResMut, Resource},
    world::EntityMut,
};
use bevy_hierarchy::BuildWorldChildren;
use bevy_math::{Mat4, Quat, Vec3};
use bevy_reflect::{std_traits::ReflectDefault, FromReflect, Reflect, Uuid};
use bevy_render::{
    camera::{Camera, CameraProjection, CameraRenderGraph, RenderTarget},
    prelude::VisibilityBundle,
    primitives::Frustum,
    view::VisibleEntities,
};
use bevy_transform::{
    components::{GlobalTransform, Transform},
    TransformBundle,
};
//  mostly copied from https://github.com/blaind/bevy_openxr/tree/main/crates/bevy_openxr/src/render_graph/camera
use openxr::{Fovf, Quaternionf, Vector3f, View};

use self::xrcameraplugin::{XrCameraLeftMarker, XrCameraRightMarker};
pub mod xrcameraplugin;

#[derive(Bundle)]
pub struct XRCameraBundle<M: Component> {
    pub camera: Camera,
    pub xr_projection: XRProjection,
    pub visible_entities: VisibleEntities,
    pub transform: Transform,
    pub global_transform: GlobalTransform,
    pub frustum: Frustum,
    pub marker: M,

    //  convince bevy to render this camera with 3d pipeline
    pub camera3d: Camera3d,
    pub camera_render_graph: CameraRenderGraph,
}

impl<M: Component + Default> Default for XRCameraBundle<M> {
    fn default() -> Self {
        Self {
            camera: Default::default(),
            xr_projection: Default::default(),
            visible_entities: Default::default(),
            transform: Default::default(),
            global_transform: Default::default(),
            frustum: Default::default(),
            marker: Default::default(),
            camera3d: Default::default(),
            camera_render_graph: CameraRenderGraph::new(core_3d::graph::NAME),
        }
    }
}

#[derive(Debug, Clone, Component, Reflect, FromReflect)]
#[reflect(Component, Default)]
pub struct XRProjection {
    pub near: f32,
    pub far: f32,
    #[reflect(ignore)]
    pub fov: Fovf,
}

impl Default for XRProjection {
    fn default() -> Self {
        Self {
            near: 0.1,
            far: 1000.,
            fov: Default::default(),
        }
    }
}

impl XRProjection {
    pub fn new(near: f32, far: f32, fov: Fovf) -> Self {
        XRProjection { near, far, fov }
    }
}

impl CameraProjection for XRProjection {
    // =============================================================================
    // math code adapted from
    // https://github.com/KhronosGroup/OpenXR-SDK-Source/blob/master/src/common/xr_linear.h
    // Copyright (c) 2017 The Khronos Group Inc.
    // Copyright (c) 2016 Oculus VR, LLC.
    // SPDX-License-Identifier: Apache-2.0
    // =============================================================================
    fn get_projection_matrix(&self) -> Mat4 {
        //  symmetric perspective for debugging
        // let x_fov = (self.fov.angle_left.abs() + self.fov.angle_right.abs());
        // let y_fov = (self.fov.angle_up.abs() + self.fov.angle_down.abs());
        // return Mat4::perspective_infinite_reverse_rh(y_fov, x_fov / y_fov, self.near);

        let fov = self.fov;
        let is_vulkan_api = false; // FIXME wgpu probably abstracts this
        let near_z = self.near;
        let far_z = -1.; //   use infinite proj
                         // let far_z = self.far;

        let tan_angle_left = fov.angle_left.tan();
        let tan_angle_right = fov.angle_right.tan();

        let tan_angle_down = fov.angle_down.tan();
        let tan_angle_up = fov.angle_up.tan();

        let tan_angle_width = tan_angle_right - tan_angle_left;

        // Set to tanAngleDown - tanAngleUp for a clip space with positive Y
        // down (Vulkan). Set to tanAngleUp - tanAngleDown for a clip space with
        // positive Y up (OpenGL / D3D / Metal).
        // const float tanAngleHeight =
        //     graphicsApi == GRAPHICS_VULKAN ? (tanAngleDown - tanAngleUp) : (tanAngleUp - tanAngleDown);
        let tan_angle_height = if is_vulkan_api {
            tan_angle_down - tan_angle_up
        } else {
            tan_angle_up - tan_angle_down
        };

        // Set to nearZ for a [-1,1] Z clip space (OpenGL / OpenGL ES).
        // Set to zero for a [0,1] Z clip space (Vulkan / D3D / Metal).
        // const float offsetZ =
        //     (graphicsApi == GRAPHICS_OPENGL || graphicsApi == GRAPHICS_OPENGL_ES) ? nearZ : 0;
        // FIXME handle enum of graphics apis
        let offset_z = 0.;

        let mut cols: [f32; 16] = [0.0; 16];

        if far_z <= near_z {
            // place the far plane at infinity
            cols[0] = 2. / tan_angle_width;
            cols[4] = 0.;
            cols[8] = (tan_angle_right + tan_angle_left) / tan_angle_width;
            cols[12] = 0.;

            cols[1] = 0.;
            cols[5] = 2. / tan_angle_height;
            cols[9] = (tan_angle_up + tan_angle_down) / tan_angle_height;
            cols[13] = 0.;

            cols[2] = 0.;
            cols[6] = 0.;
            cols[10] = -1.;
            cols[14] = -(near_z + offset_z);

            cols[3] = 0.;
            cols[7] = 0.;
            cols[11] = -1.;
            cols[15] = 0.;

            //  bevy uses the _reverse_ infinite projection
            //  https://dev.theomader.com/depth-precision/
            let z_reversal = Mat4::from_cols_array_2d(&[
                [1f32, 0., 0., 0.],
                [0., 1., 0., 0.],
                [0., 0., -1., 0.],
                [0., 0., 1., 1.],
            ]);

            return z_reversal * Mat4::from_cols_array(&cols);
        } else {
            // normal projection
            cols[0] = 2. / tan_angle_width;
            cols[4] = 0.;
            cols[8] = (tan_angle_right + tan_angle_left) / tan_angle_width;
            cols[12] = 0.;

            cols[1] = 0.;
            cols[5] = 2. / tan_angle_height;
            cols[9] = (tan_angle_up + tan_angle_down) / tan_angle_height;
            cols[13] = 0.;

            cols[2] = 0.;
            cols[6] = 0.;
            cols[10] = -(far_z + offset_z) / (far_z - near_z);
            cols[14] = -(far_z * (near_z + offset_z)) / (far_z - near_z);

            cols[3] = 0.;
            cols[7] = 0.;
            cols[11] = -1.;
            cols[15] = 0.;
        }

        Mat4::from_cols_array(&cols)
    }

    fn update(&mut self, _width: f32, _height: f32) {}

    fn far(&self) -> f32 {
        self.far
    }
}

#[derive(Resource)]
pub struct XrViews(pub Vec<View>);

pub fn update_xrcamera_view(
    mut cam: Query<(&mut XRProjection, &mut Transform, &Eye)>,
    mut xr_cam: Query<(&mut Transform, &XrCameras), Without<Eye>>,
    views: Option<ResMut<XrViews>>,
) {
    let views = match &views {
        Some(views) => &views.0,
        None => return,
    };
    let midpoint = (views.get(0).unwrap().pose.position.to_vec3()
        + views.get(1).unwrap().pose.position.to_vec3())
        / 2.;
    xr_cam.single_mut().0.translation = midpoint;

    let left_rot = views.get(0).unwrap().pose.orientation.to_quat();
    let right_rot = views.get(1).unwrap().pose.orientation.to_quat();
    let mid_rot = if left_rot.dot(right_rot) >= 0. {
        left_rot.slerp(right_rot, 0.5)
    } else {
        right_rot.slerp(left_rot, 0.5)
    };
    xr_cam.single_mut().0.rotation = mid_rot;

    /*
        TODO: figure out transform hierarchy for XrCameras
        maybe have a parent object:
        XrPlayer -- transform set by developer
        |
        V
        XrCameras -- transform set as midpoint/midrotation of two openXR views,
        |           used for developer to understand relative head position
        |
        V
        [XrCamera::left, XrCamera::right] -- transform set as individual views, used for rendering

    */
    for (mut projection, mut transform, eye) in cam.iter_mut() {
        let view_idx = match eye {
            Eye::Left => 0,
            Eye::Right => 1,
        };
        let view = views.get(view_idx).unwrap();

        projection.fov = view.fov;

        transform.rotation = view.pose.orientation.to_quat();
        let pos = view.pose.position;
        transform.translation = pos.to_vec3();
    }
}

#[derive(Component)]
pub struct XrPawn {}

impl XrPawn {
    pub fn spawn(mut e: EntityMut, left_id: Uuid, right_id: Uuid) {
        e.with_children(|pawn| {
            pawn.spawn(XrCameras {}).insert(TransformBundle::default());
            pawn.spawn(XRCameraBundle {
                camera: Camera {
                    target: RenderTarget::TextureView(left_id),
                    is_active: true,
                    ..Default::default()
                },
                marker: XrCameraLeftMarker,
                ..Default::default()
            })
            .insert(Eye::Left);
            pawn.spawn(XRCameraBundle {
                camera: Camera {
                    target: RenderTarget::TextureView(right_id),
                    is_active: true,
                    ..Default::default()
                },
                marker: XrCameraRightMarker,
                ..Default::default()
            })
            .insert(Eye::Right);
        })
        .insert(Self {})
        .insert(TransformBundle::default())
        .insert(VisibilityBundle::default());
    }
}

#[derive(Component)]
pub struct XrCameras {}

#[derive(Component, Debug)]
pub enum Eye {
    Left,
    Right,
}

pub trait Vec3Conv {
    fn to_vec3(&self) -> Vec3;
}

impl Vec3Conv for Vector3f {
    fn to_vec3(&self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}

pub trait QuatConv {
    fn to_quat(&self) -> Quat;
}

impl QuatConv for Quaternionf {
    fn to_quat(&self) -> Quat {
        Quat::from_xyzw(self.x, self.y, self.z, self.w)
    }
}
