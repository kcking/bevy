/// This is a Work-In-Progress module to run a winit EventLoop alongside the
/// OpenXR custom runner. This will allow editor support at the same time as a
/// simulator or PCVR headset is runnning.
use bevy_app::{App, AppExit};
use bevy_ecs::event::ManualEventReader;
use bevy_window::{CreateWindow, RequestRedraw, Windows};
use bevy_winit::{WinitCreateWindowReader, WinitPersistentState};
use winit::{
    event::Event,
    event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget},
    platform::run_return::EventLoopExtRunReturn,
};

#[derive(Default)]
pub struct State {
    app_exit_reader: ManualEventReader<AppExit>,
    redraw_reader: ManualEventReader<RequestRedraw>,
    persistent: WinitPersistentState,
}

impl State {
    pub fn new() -> State {
        State {
            ..Default::default()
        }
    }
}

pub fn init_window(app: &mut App) {
    let event_loop = app
        .world
        .remove_non_send_resource::<EventLoop<()>>()
        .unwrap();
    let mut create_window_reader = app
        .world
        .remove_resource::<WinitCreateWindowReader>()
        .unwrap();
    bevy_winit::handle_create_window_events(
        &mut app.world,
        &event_loop,
        &mut create_window_reader.0,
    );
    app.insert_non_send_resource(event_loop);
    app.insert_resource(create_window_reader);
}
pub fn run_event_loop(state: State, app: &mut App) -> State {
    //  unpack and repack State so we can have mutable access to multiple fields
    let State {
        mut app_exit_reader,
        mut redraw_reader,
        mut persistent,
    } = state;

    let mut event_loop = app
        .world
        .remove_non_send_resource::<EventLoop<()>>()
        .unwrap();

    let mut create_window_reader = app
        .world
        .remove_resource::<WinitCreateWindowReader>()
        .unwrap();

    event_loop.run_return(
        |event: Event<()>,
         event_loop: &EventLoopWindowTarget<()>,
         control_flow: &mut ControlFlow| {
            bevy_winit::winit_event_handler(
                event,
                event_loop,
                control_flow,
                app,
                &mut persistent,
                &mut create_window_reader.0,
                &mut app_exit_reader,
                &mut redraw_reader,
            );
        },
    );

    app.insert_non_send_resource(event_loop);
    app.insert_resource(create_window_reader);

    State {
        app_exit_reader,
        redraw_reader,
        persistent,
    }
}
