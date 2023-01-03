use xr::{EnvironmentBlendMode, FrameWaiter, ViewConfigurationType};

use crate::*;

pub fn setup_xrcontext_and_graphics(app: &mut App) {
    app.insert_resource(WgpuSettings {
        backends: Some(Backends::VULKAN),
        ..Default::default()
    });
    #[cfg(feature = "simulator")]
    {
        let mut event_loop = app
            .world
            .remove_non_send_resource::<EventLoop<()>>()
            .unwrap();
        bevy_openxr_simulator::simulator::pre_graphics_init(&mut event_loop);
        app.insert_non_send_resource(event_loop);
    }

    if !app.world.contains_resource::<OpenXrContext>() {
        let context =
            OpenXrContext::new(OpenXrFormFactor::HeadMountedDisplay).unwrap_or_else(|_| {
                match OpenXrContext::new(OpenXrFormFactor::Handheld) {
                    Ok(context) => context,
                    // In case OpenXR is suported, there should be always at least one supported
                    // form factor. If "Handheld" is unsupported, "HeadMountedDisplay" is
                    // supported (but in this case unavailable).
                    Err(
                        OpenXrError::UnsupportedFormFactor | OpenXrError::UnavailableFormFactor,
                    ) => panic!(
                        "OpenXR: No available form factors. Consider manually handling {}",
                        "the creation of the OpenXrContext resource."
                    ),
                    Err(OpenXrError::InstanceCreation(sys::Result::ERROR_RUNTIME_FAILURE)) => {
                        panic!(
                            "OpenXR: Failed to create OpenXrContext: {:?}\n{} {}",
                            sys::Result::ERROR_RUNTIME_FAILURE,
                            "Is your headset connected? Also, consider manually handling",
                            "the creation of the OpenXrContext resource."
                        )
                    }
                    Err(e) => panic!(
                        "OpenXR: Failed to create OpenXrContext: {:?}\n{} {}",
                        e,
                        "Consider manually handling",
                        "the creation of the OpenXrContext resource."
                    ),
                }
            });
        app.world.insert_resource(context);
    }

    let mut context = app.world.get_resource_mut::<OpenXrContext>().unwrap();
    let mut graphics_context = context.graphics_context.take().unwrap();
    println!("got graphics context");

    let instance = RenderInstance(graphics_context.instance.take().unwrap());
    let dev = renderer::RenderDevice::from(graphics_context.device.clone());
    let queue = renderer::RenderQueue(graphics_context.queue.clone());
    let adapter_info = renderer::RenderAdapterInfo(graphics_context.adapter_info.clone());
    let adapter = renderer::RenderAdapter(graphics_context.adapter.clone());

    {
        app.insert_resource(dev)
            .insert_resource(queue)
            .insert_resource(adapter_info)
            .insert_resource(adapter)
            .insert_resource(instance)
    };

    app.insert_resource::<XrGraphicsContext>(graphics_context)
        .set_runner(runner);

    app.insert_resource(Msaa { samples: 1 });
}

#[derive(Resource)]
pub struct XrRunnerState {
    pub(crate) tracking_context: Arc<OpenXrTrackingContext>,
    pub(crate) view_type: ViewConfigurationType,
    pub(crate) app_exit_event_reader: ManualEventReader<AppExit>,
    pub(crate) interaction_context: InteractionContext,
    pub(crate) frame_waiter: FrameWaiter,
    pub(crate) frame_stream: xr::FrameStream<xr::Vulkan>,
    pub(crate) blend_mode: EnvironmentBlendMode,
    pub(crate) next_vsync_time: Arc<RwLock<xr::Time>>,
    pub(crate) stage: xr::Space,
    pub(crate) vk_session: xr::Session<xr::Vulkan>,
    pub(crate) left_id: Uuid,
    pub(crate) right_id: Uuid,
    pub(crate) xr_context: OpenXrContext,
}

pub fn setup_other_xr(app: &mut App) -> XrRunnerState {
    let ctx = app.world.remove_resource::<OpenXrContext>().unwrap();
    #[cfg(feature = "winit_loop")]
    {
        app.world.init_resource::<WinitSettings>();
        app.world.resource_mut::<WinitSettings>().return_from_run = true;
    }
    #[cfg(feature = "simulator")]
    {
        let mut event_loop = app
            .world
            .remove_non_send_resource::<EventLoop<()>>()
            .unwrap();
        bevy_openxr_simulator::simulator::pre_init(&mut event_loop);
        app.insert_non_send_resource(event_loop);
    }

    app.world
        .insert_resource(XrInstanceRes(ctx.instance.clone()));

    let mut app_exit_event_reader = ManualEventReader::default();

    let interaction_mode = if ctx.form_factor == xr::FormFactor::HEAD_MOUNTED_DISPLAY {
        XrInteractionMode::WorldSpace
    } else {
        XrInteractionMode::ScreenSpace
    };
    app.world.insert_resource(interaction_mode);

    // Find the available session modes
    let available_session_modes = [
        XrSessionMode::ImmersiveVR,
        XrSessionMode::ImmersiveAR,
        XrSessionMode::InlineVR,
        XrSessionMode::InlineAR,
    ]
    .iter()
    .filter_map(|mode| get_system_info(&ctx.instance, ctx.system, *mode).map(|_| *mode))
    .collect();

    app.world
        .insert_resource(XrSystem::new(available_session_modes));
    println!("inserted XrSystem");

    let mut xr_system = app.world.get_resource_mut::<XrSystem>().unwrap();
    setup_interaction(&mut xr_system);

    let mode = xr_system.selected_session_mode();
    let bindings = xr_system.action_set();
    dbg!(bindings.iter().map(|b| &b.profile).collect::<Vec<_>>());

    let interaction_context = InteractionContext::new(&ctx.instance, bindings);

    // Remove XrSystem. The user cannot make any more changes to the session mode.
    // todo: when the lifecycle API is implemented, allow the user to change the session mode at any
    // moment.
    // app.world.remove_resource::<XrSystem>();

    let (view_type, blend_mode) = get_system_info(&ctx.instance, ctx.system, mode).unwrap();

    let environment_blend_mode = match blend_mode {
        xr::EnvironmentBlendMode::OPAQUE => XrEnvironmentBlendMode::Opaque,
        xr::EnvironmentBlendMode::ALPHA_BLEND => XrEnvironmentBlendMode::AlphaBlend,
        xr::EnvironmentBlendMode::ADDITIVE => XrEnvironmentBlendMode::Additive,
        _ => unreachable!(),
    };
    app.world.insert_resource(environment_blend_mode);

    let (vk_session, session, _graphics_session, mut frame_waiter, mut frame_stream) =
        match ctx.graphics_handles {
            GraphicsContextHandles::Vulkan {
                ref instance,
                physical_device,
                ref device,
                queue_family_index,
                queue_index,
            } => {
                let (session, frame_waiter, frame_stream) = unsafe {
                    ctx.instance
                        .create_session(
                            ctx.system,
                            &xr::vulkan::SessionCreateInfo {
                                instance: instance.handle().as_raw() as *const _,
                                physical_device: physical_device.as_raw() as *const _,
                                device: device.handle().as_raw() as *const _,
                                queue_family_index,
                                queue_index,
                            },
                        )
                        .unwrap()
                };
                (
                    session.clone(),
                    session.clone().into_any_graphics(),
                    SessionBackend::Vulkan(session),
                    frame_waiter,
                    frame_stream,
                )
            }
        };

    let session = OpenXrSession {
        inner: Some(session),
        _wgpu_device: ctx.wgpu_device.clone(),
    };

    // The user can have a limited access to the OpenXR session using OpenXrSession, which is
    // clonable but safe because of the _wgpu_device internal handle.
    app.world.insert_resource(session.clone());

    session
        .attach_action_sets(&[&interaction_context.action_set.lock()])
        .unwrap();

    let tracking_context = Arc::new(OpenXrTrackingContext::new(
        &ctx.instance,
        ctx.system,
        &interaction_context,
        session.clone(),
    ));

    let next_vsync_time = Arc::new(RwLock::new(xr::Time::from_nanos(0)));

    let tracking_source = TrackingSource {
        view_type,
        action_set: interaction_context.action_set.clone(),
        session: session.clone(),
        context: tracking_context.clone(),
        next_vsync_time: next_vsync_time.clone(),
    };

    app.world.init_resource::<XrActionSet>();
    app.world
        .insert_resource(OpenXrTrackingContextRes(tracking_context.clone()));
    app.world
        .insert_resource(XrTrackingSource::new(Box::new(tracking_source)));

    // todo: use these views limits and recommendations
    let _views = ctx
        .instance
        .enumerate_view_configuration_views(ctx.system, view_type)
        .unwrap();

    let stage = session
        .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
        .unwrap();

    let left_id = Uuid::new_v4();
    let right_id = Uuid::new_v4();
    XrPawn::spawn(app.world.spawn_empty(), left_id, right_id);

    XrRunnerState {
        tracking_context,
        view_type,
        app_exit_event_reader,
        interaction_context,
        frame_stream,
        frame_waiter,
        blend_mode,
        next_vsync_time,
        stage,
        vk_session,
        left_id,
        right_id,
        xr_context: ctx,
    }
}
