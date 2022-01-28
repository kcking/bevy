use ash::vk;
use bevy_math::{Quat, Vec3};
use bevy_utils::Duration;
use openxr as xr;

pub fn from_duration(duration: Duration) -> xr::Duration {
    xr::Duration::from_nanos(duration.as_nanos() as _)
}

pub fn to_vec3(v: xr::Vector3f) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub fn to_quat(q: xr::Quaternionf) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w)
}

use bevy_math::UVec2;

pub trait Size2D {
    fn bevy(&self) -> UVec2;
    fn wgpu(&self) -> wgpu::Extent3d;
    fn vk(&self) -> vk::Extent2D;
    fn xr(&self) -> xr::Rect2Di;
}

pub trait ToUVec2 {
    fn to_uvec2(&self) -> UVec2;
}

impl ToUVec2 for wgpu::Extent3d {
    fn to_uvec2(&self) -> UVec2 {
        UVec2::new(self.width, self.height)
    }
}

impl ToUVec2 for vk::Extent2D {
    fn to_uvec2(&self) -> UVec2 {
        UVec2::new(self.width, self.height)
    }
}

impl ToUVec2 for UVec2 {
    fn to_uvec2(&self) -> UVec2 {
        *self
    }
}

impl<T: ToUVec2> Size2D for T {
    fn bevy(&self) -> UVec2 {
        self.to_uvec2()
    }

    fn wgpu(&self) -> wgpu::Extent3d {
        let uv2 = self.to_uvec2();
        wgpu::Extent3d {
            width: uv2.x,
            height: uv2.y,
            depth_or_array_layers: 1,
        }
    }

    fn vk(&self) -> vk::Extent2D {
        let uv2 = self.to_uvec2();
        vk::Extent2D {
            width: uv2.x,
            height: uv2.y,
        }
    }

    fn xr(&self) -> xr::Rect2Di {
        let uv2 = self.to_uvec2();
        xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di {
                width: uv2.x as _,
                height: uv2.y as _,
            },
        }
    }
}
