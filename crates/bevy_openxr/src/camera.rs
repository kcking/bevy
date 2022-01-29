use bevy_math::Mat4;
use openxr::Fovf;

// =============================================================================
// math code adapted from
// https://github.com/KhronosGroup/OpenXR-SDK-Source/blob/master/src/common/xr_linear.h
// Copyright (c) 2017 The Khronos Group Inc.
// Copyright (c) 2016 Oculus VR, LLC.
// SPDX-License-Identifier: Apache-2.0
// =============================================================================
pub fn get_projection_matrix_fov(fov: &Fovf) -> Mat4 {
    let is_vulkan_api = false; // FIXME wgpu probably abstracts this
    let near_z = self.near;
    let far_z = self.far;

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
    let offset_z = if !is_vulkan_api { near_z } else { 0. };

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

#[derive(Component)]
pub struct XrProjection {}
