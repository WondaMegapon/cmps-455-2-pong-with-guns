use hecs::*;
use macroquad::prelude::*;
use rodio::*;

// And our constants.
const MAX_VOLUME: f32 = 0.1;

macro_rules! play_audio {
    ($sink:ident, $file:expr $(,)?, $volume:expr $(,)?, $speed:expr $(,)?) => {
        $sink.skip_one();
        $sink.append(
            Decoder::new_wav(std::io::Cursor::new(&include_bytes!($file)))
                .unwrap()
                .amplify($volume)
                .speed($speed),
        );
    };
}

// Tracking the phases of a game.
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Phase {
    Start,
    Ongoing,
    LeftWin,
    RightWin,
}

// Implementing the default phase.
impl Default for Phase {
    fn default() -> Self {
        Phase::Start
    }
}

// A component to store an objects position and velocity.
#[derive(Default, Clone, Copy)]
struct Transform {
    position: (f32, f32),
    velocity: (f32, f32),
}

// A component to store an object's bounds (for collision testing.)
#[derive(Default, Clone, Copy)]
struct Bounds(f32, f32); // (radius, length) Mostly here as a reminder.

#[derive(Default, Clone)]
struct Controls {
    up: Vec<KeyCode>,
    left: Vec<KeyCode>,
    down: Vec<KeyCode>,
    right: Vec<KeyCode>,
}

// For tracking the controls of a given entity. (Also bullet cooldowns)
enum ControlType {
    AI(f64),
    Player(Controls, f64),
}

// The ball!
#[derive(Default, Clone, Copy)]
struct Ball {
    radius: f32,
    speed: f32,
}

#[derive(Default, Clone, Copy)]
struct Bullet {
    radius: f32,
}

// The game state as a whole.
#[derive(Default, Clone, Copy)]
struct GameState {
    phase: Phase,
    left_score: i32,
    right_score: i32,
    intensity: f32,
    target_color: Color,
    current_color: Color,
    hitstun: i32,
}

// Creating a constructor for it.
impl GameState {
    fn new() -> Self {
        GameState {
            phase: Phase::Start,
            left_score: 0,
            right_score: 0,
            intensity: 0.0,
            target_color: BLACK,
            current_color: BLACK,
            hitstun: 0,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Particle {
    position: (f32, f32),
    velocity: (f32, f32),
    size: f32,
    color: Color,
    birthtime: f64,
    deathtime: f64,
}

#[derive(Default, Clone)]
struct ParticleStorage {
    particles_container: Vec<Particle>,
}

impl ParticleStorage {
    fn new() -> Self {
        Self {
            particles_container: Vec::new(),
        }
    }

    fn create_particle(
        &mut self,
        count: i32,
        position: (f32, f32),
        velocity: (f32, f32),
        size: f32,
        color: Color,
        age: f64,
        position_variance: (f32, f32),
        velocity_variance: (f32, f32),
        size_variance: f32,
        age_variance: f64,
    ) {
        let curr_time = macroquad::time::get_time();
        for _i in 0..count {
            self.particles_container.push(Particle {
                position: (
                    position.0
                        + rand::RandomRange::gen_range(-position_variance.0, position_variance.0),
                    position.1
                        + rand::RandomRange::gen_range(-position_variance.1, position_variance.1),
                ),
                velocity: (
                    velocity.0
                        + rand::RandomRange::gen_range(-velocity_variance.0, velocity_variance.0),
                    velocity.1
                        + rand::RandomRange::gen_range(-velocity_variance.1, velocity_variance.1),
                ),
                size: size + rand::RandomRange::gen_range(-size_variance, size_variance),
                color: color,
                birthtime: curr_time,
                deathtime: curr_time
                    + age
                    + rand::RandomRange::gen_range(-age_variance, age_variance),
            })
        }
    }
}

fn world_reset(world: &mut World) {
    world.clear(); // Resetting the world.
                   // Our left paddle.
    world.spawn((
        Transform {
            position: (64.0, screen_height() / 2.0),
            velocity: (0.0, 0.0),
        },
        Bounds(16.0, 64.0),
        ControlType::Player(
            Controls {
                up: vec![KeyCode::W],
                left: vec![KeyCode::A],
                down: vec![KeyCode::S],
                right: vec![KeyCode::D],
            },
            0.0,
        ),
    ));
    // Our right paddle.
    world.spawn((
        Transform {
            position: (screen_width() - 64.0, screen_height() / 2.0),
            velocity: (0.0, 0.0),
        },
        Bounds(16.0, 64.0),
        ControlType::AI(0.0),
    ));
}

fn square_distance(x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    (x1 - x2).powf(2.0) + (y1 - y2).powf(2.0)
}

// Returns the squared distance between point c and segment ab
fn square_distance_point_segment(a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> f32 {
    let ab = (b.0 - a.0, b.1 - a.1); // Getting our distance vectors.
    let ac = (c.0 - a.0, c.1 - a.1);
    let bc = (c.0 - b.0, c.1 - b.1);
    let e = ac.0 * ab.0 + ac.1 * ab.1; // Getting the dot product for the central thingy.
    if e <= 0.0 {
        return ac.0 * ac.0 + ac.1 * ac.1;
    } // Handle cases where c projects outside ab
    let f = ab.0 * ab.0 + ab.1 * ab.1;
    if e >= f {
        return bc.0 * bc.0 + bc.1 * bc.1;
    } // Handle cases where c projects onto ab
    (ac.0 * ac.0 + ac.1 * ac.1) - e * e / f
}

fn test_sphere_capsule(sphere: (&Transform, &Ball), capsule: (&Transform, &Bounds)) -> bool {
    // Compute (squared) distance between sphere center and capsule line segment
    let dist2 = square_distance_point_segment(
        (
            capsule.0.position.0,
            capsule.0.position.1 + (capsule.1 .1 / 2.0),
        ),
        (
            capsule.0.position.0,
            capsule.0.position.1 - (capsule.1 .1 / 2.0),
        ),
        sphere.0.position,
    );
    // If (squared) distance smaller than (squared) sum of radii, they collide
    dist2 <= (sphere.1.radius + capsule.1 .0).powf(2.0)
}

// Setting Window Configurations.
fn config() -> Conf {
    Conf {
        window_title: "Pong with Guns".to_string(),
        fullscreen: true,
        ..Default::default()
    }
}

// Main!
#[macroquad::main(config)]
async fn main() {
    let mut game_state = GameState::new(); // Creating the new gamestate.
    let mut world = World::new(); // For storing all of our entities. :)
    let mut particles = ParticleStorage::new(); // Here is this funny thing.
    let mut frame_count = 0_u64;

    // Music stuff.
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let sink_bass = Sink::try_new(&stream_handle).unwrap();
    let sink_drums = Sink::try_new(&stream_handle).unwrap();
    let sink_synth = Sink::try_new(&stream_handle).unwrap();
    let sink_vocals = Sink::try_new(&stream_handle).unwrap();
    let sink_sfx = Sink::try_new(&stream_handle).unwrap();

    let mut target_volume_bass;
    let mut target_volume_drums;
    let mut target_volume_synth;
    let mut target_volume_vocals;

    let mut current_volume_bass = 0.0;
    let mut current_volume_drums = 0.0;
    let mut current_volume_synth = 0.0;
    let mut current_volume_vocals = 0.0;

    particles.create_particle(
        125,
        (screen_width() / 2.0, screen_height() / 2.0),
        (0.0, 0.4),
        2.0,
        WHITE,
        60.0,
        (screen_width() / 2.0, screen_height() / 2.0),
        (0.0, 0.2),
        0.0,
        0.0,
    );

    world_reset(&mut world);

    'main: loop {
        // And for frame time.
        let current_time = macroquad::time::get_time();
        frame_count += 1; // This too.
        let screenshake_offset = (
            (frame_count as f32).sin() * game_state.hitstun as f32 / 2.0,
            (frame_count as f32 * 0.1).sin() * game_state.hitstun as f32 / 2.0,
        );

        // Audio control, 'cause music is important.
        target_volume_bass = 1.0_f32;
        target_volume_drums = (((game_state.intensity / 5.0) - 0.8)
            * (game_state.phase == Phase::Ongoing) as i32 as f32)
            .clamp(0.0, 1.0);
        target_volume_synth = (((game_state.intensity / 5.0) - 1.6)
            * (game_state.phase == Phase::Ongoing) as i32 as f32)
            .clamp(0.0, 1.0);
        target_volume_vocals = (((game_state.intensity / 5.0) - 3.4)
            * (game_state.phase == Phase::Ongoing) as i32 as f32)
            .clamp(0.0, 1.0);

        // Updating targets.
        current_volume_bass = (current_volume_bass * 0.9) + (target_volume_bass * 0.1);
        current_volume_drums = (current_volume_drums * 0.9) + (target_volume_drums * 0.1);
        current_volume_synth = (current_volume_synth * 0.9) + (target_volume_synth * 0.1);
        current_volume_vocals = (current_volume_vocals * 0.9) + (target_volume_vocals * 0.1);

        // Actually setting the values
        sink_bass.set_volume(current_volume_bass.clamp(0.0, MAX_VOLUME));
        sink_drums.set_volume(current_volume_drums.clamp(0.0, MAX_VOLUME));
        sink_synth.set_volume(current_volume_synth.clamp(0.0, MAX_VOLUME));
        sink_vocals.set_volume(current_volume_vocals.clamp(0.0, MAX_VOLUME));

        // Refreshing our samples if its empty.
        if sink_vocals.empty() {
            let music_bass = Decoder::new_wav(std::io::Cursor::new(&include_bytes!(
                "assets/music/Bass.wav"
            )))
            .unwrap();
            let music_drums = Decoder::new_wav(std::io::Cursor::new(&include_bytes!(
                "assets/music/Drums.wav"
            )))
            .unwrap();
            let music_synth = Decoder::new_wav(std::io::Cursor::new(&include_bytes!(
                "assets/music/Synth.wav"
            )))
            .unwrap();
            let music_vocals = Decoder::new_wav(std::io::Cursor::new(&include_bytes!(
                "assets/music/Vocals.wav"
            )))
            .unwrap();

            sink_bass.append(music_bass);
            sink_drums.append(music_drums);
            sink_synth.append(music_synth);
            sink_vocals.append(music_vocals);
        }

        // Handling Rendering.
        //
        // Clearing our background.
        game_state.target_color = Color {
            r: game_state.intensity / 400.0
                + (game_state.hitstun as f32 / 10.0).clamp(0.0, 0.1)
                + (game_state.left_score as f32 / 50.0),
            g: game_state.intensity / 400.0 + (game_state.hitstun as f32 / 10.0).clamp(0.0, 0.1),
            b: game_state.intensity / 400.0
                + (game_state.hitstun as f32 / 10.0).clamp(0.0, 0.1)
                + (game_state.right_score as f32 / 50.0),
            a: 1.0,
        };
        game_state.current_color = Color {
            r: game_state.current_color.r * 0.9 + game_state.target_color.r * 0.1,
            g: game_state.current_color.g * 0.9 + game_state.target_color.g * 0.1,
            b: game_state.current_color.b * 0.9 + game_state.target_color.b * 0.1,
            a: 1.0,
        };
        clear_background(game_state.current_color);

        // Particles, since these are background items.
        particles.particles_container.iter_mut().for_each(|part| {
            draw_circle(
                part.position.0,
                part.position.1,
                clamp(
                    part.size
                        * ((current_time - part.deathtime) / (part.birthtime - part.deathtime))
                            .clamp(0.0, 1.0) as f32,
                    0.0,
                    f32::MAX,
                ),
                Color {
                    r: part.color.r,
                    g: part.color.g,
                    b: part.color.b,
                    a: part.color.a,
                },
            );

            part.position = (
                part.position.0 + part.velocity.0,
                part.position.1 + part.velocity.1,
            )
        });
        particles
            .particles_container
            .retain(|&part| part.deathtime > current_time);

        // Current Phase text.
        {
            let phase_text = match game_state.phase {
                Phase::Start => "Waiting for Spacebar.",
                Phase::Ongoing => "Game ahoy!",
                Phase::LeftWin => "Left wins!",
                Phase::RightWin => "Right wins!",
            };
            let text_horizontal_pos =
                (screen_width() / 2.0) - (measure_text(&phase_text, None, 32, 1.0).width / 2.0);
            draw_text(
                &phase_text,
                text_horizontal_pos + screenshake_offset.0,
                64.0 + screenshake_offset.1,
                32.0,
                WHITE,
            );
            let score_text = format!("{} - {}", game_state.left_score, game_state.right_score);
            let text_horizontal_pos =
                (screen_width() / 2.0) - (measure_text(&score_text, None, 32, 1.0).width / 2.0);
            draw_text(
                &score_text,
                text_horizontal_pos + screenshake_offset.0,
                screen_height() - 64.0 + screenshake_offset.1,
                32.0,
                WHITE,
            );
            let speed_text = format!("{}", game_state.intensity.round().abs());
            let text_horizontal_pos =
                (screen_width() / 2.0) - (measure_text(&speed_text, None, 32, 1.0).width / 2.0);
            draw_text(
                &speed_text,
                text_horizontal_pos + screenshake_offset.0,
                screen_height() - 32.0 + screenshake_offset.1,
                32.0,
                WHITE,
            );
        }

        // DRAWING SYSTEM
        for (_id, (transform, _ball)) in world.query_mut::<(&Transform, &Bullet)>() {
            // Drawing the bullet.
            draw_circle(
                transform.position.0 + screenshake_offset.0,
                transform.position.1 + screenshake_offset.1,
                8.0,
                BLACK,
            );
            draw_circle(
                transform.position.0 + screenshake_offset.0,
                transform.position.1 + screenshake_offset.1,
                4.0,
                WHITE,
            );
        }

        //
        // Handling balls.
        for (_id, (transform, ball)) in world.query_mut::<(&Transform, &Ball)>() {
            // Drawing the ball outline.
            draw_circle_lines(
                transform.position.0 + screenshake_offset.0,
                transform.position.1 + screenshake_offset.1,
                ball.radius,
                2.0,
                WHITE,
            );
            // Drawing the ball.
            draw_circle(
                transform.position.0 + screenshake_offset.0,
                transform.position.1 + screenshake_offset.1,
                2.0,
                BLACK,
            );
        }

        // Handling Paddles
        for (_id, (transform, bounds)) in world.query_mut::<(&Transform, &Bounds)>() {
            draw_rectangle(
                transform.position.0 - bounds.0,
                transform.position.1 - bounds.1,
                bounds.0 * 2.0,
                bounds.1 * 2.0,
                BLACK,
            );
            draw_rectangle_lines(
                transform.position.0 - bounds.0,
                transform.position.1 - bounds.1,
                bounds.0 * 2.0,
                bounds.1 * 2.0,
                4.0,
                WHITE,
            );
        }

        // Handling Tutorial Text
        if game_state.phase != Phase::Ongoing {
            for (_id, (transform, controls, bounds)) in
                world.query_mut::<(&Transform, &ControlType, &Bounds)>()
            {
                let color = if ((current_time * 1.1) % 2.0) < 1.0 {
                    WHITE
                } else {
                    GRAY
                };
                match controls {
                    ControlType::Player(x, _c) => {
                        draw_text(
                            &format!("{:?}", &x.up[0]),
                            transform.position.0 - 8.0,
                            transform.position.1 - bounds.1 - 8.0,
                            36.0,
                            color,
                        );
                        draw_text(
                            &format!("{:?}", &x.down[0]),
                            transform.position.0 - 8.0,
                            transform.position.1 + bounds.1 + 26.0,
                            36.0,
                            color,
                        );
                        draw_text(
                            &format!("{:?}", &x.left[0]),
                            transform.position.0 - bounds.0 - 24.0,
                            transform.position.1 + 8.0,
                            36.0,
                            color,
                        );
                        draw_text(
                            &format!("{:?}", &x.right[0]),
                            transform.position.0 + bounds.0 + 8.0,
                            transform.position.1 + 8.0,
                            36.0,
                            color,
                        );
                    }
                    ControlType::AI(_c) => {
                        draw_text(
                            &"AUTO",
                            transform.position.0 - 32.0,
                            transform.position.1 - bounds.1 - 8.0,
                            36.0,
                            color,
                        );
                    }
                }
            }
        }

        // Handling Physics.
        //
        // Braced for escaping the game.
        if is_key_pressed(KeyCode::Escape) {
            break 'main;
        }

        // // Handling state changes.
        if game_state.hitstun <= 0 {
            if game_state.phase != Phase::Ongoing && is_key_pressed(KeyCode::Space) {
                // And our ball.
                let start_speed = screen_width() / 1280.0;
                world.spawn((
                    Transform {
                        position: (screen_width() / 2.0, screen_height() / 2.0),
                        velocity: (
                            start_speed
                                * (((game_state.phase != Phase::RightWin) as i32 as f32)
                                    - ((game_state.phase == Phase::RightWin) as i32 as f32)),
                            0.0,
                        ),
                    },
                    Ball {
                        radius: 16.0,
                        speed: start_speed,
                    },
                ));
                // Resetting the bounds of the paddles.
                for (_id, (_transform, bounds)) in world.query_mut::<(&Transform, &mut Bounds)>() {
                    bounds.0 = 16.0;
                    bounds.1 = 64.0;
                }
                // And finally, kicking everything off.
                game_state.phase = Phase::Ongoing;
            }

            // Let's pull a Mario 64.
            for _i in 1..4 {
                // Updating positions from velocities.
                for (_id, transform) in world.query_mut::<&mut Transform>() {
                    transform.position = (
                        clamp(
                            transform.position.0 + transform.velocity.0,
                            -16.0,
                            screen_width() + 16.0,
                        ),
                        clamp(
                            transform.position.1 + transform.velocity.1,
                            -16.0,
                            screen_height() + 16.0,
                        ),
                    );
                }

                // Processing Paddles.
                {
                    let entities = world
                        .query::<(&Transform, &Ball)>()
                        .iter()
                        .map(|(e, (&i, &b))| (e, i, b)) // Copy out of the world
                        .collect::<Vec<_>>();
                    let mut spawn_queue: Vec<(Transform, Bullet)> = Vec::new();
                    for (_id, (transform, control)) in
                        world.query_mut::<(&mut Transform, &mut ControlType)>()
                    {
                        // Slowing things down just a bit, just to ease control.
                        transform.velocity =
                            (transform.velocity.0 * 0.95, transform.velocity.1 * 0.95);

                        // Handling Controls
                        match control {
                            ControlType::Player(x, s) => {
                                transform.velocity = (
                                    transform.velocity.0,
                                    transform.velocity.1
                                        + ((is_key_down(x.down[0]) as i32 as f32)
                                            - (is_key_down(x.up[0]) as i32 as f32))
                                            * 0.3,
                                );
                                if (is_key_down(x.right[0]) ^ is_key_down(x.left[0]))
                                    && current_time > *s
                                {
                                    *s = current_time + 0.35;
                                    spawn_queue.push((
                                        Transform {
                                            position: (
                                                transform.position.0
                                                    + ((is_key_down(x.right[0]) as i32 as f32)
                                                        - (is_key_down(x.left[0]) as i32 as f32))
                                                        * 32.0,
                                                transform.position.1,
                                            ),
                                            velocity: (
                                                (((is_key_down(x.right[0]) as i32 as f32)
                                                    - (is_key_down(x.left[0]) as i32 as f32))
                                                    * 2.0),
                                                rand::RandomRange::gen_range(-0.1, 0.1),
                                            ),
                                        },
                                        Bullet { radius: 2.0 },
                                    ));
                                    play_audio!(
                                        sink_sfx,
                                        "assets/sfx/bullet_shot.wav",
                                        0.05,
                                        rand::RandomRange::gen_range(0.9, 1.0)
                                    );
                                }
                            }
                            ControlType::AI(mut _s) => {
                                if entities.first().is_some() {
                                    let (mut target, mut target_distance) = (entities[0], f32::MAX);
                                    for (id, ball_transform, ball_ball) in &entities {
                                        let temp_distance = square_distance(
                                            transform.position.0,
                                            transform.position.1,
                                            ball_transform.position.0,
                                            ball_transform.position.1,
                                        );
                                        if temp_distance < target_distance {
                                            target = (*id, *ball_transform, *ball_ball); // Setting the current target.
                                            target_distance = temp_distance;
                                        }
                                    }
                                    transform.velocity =
                                        (
                                            transform.velocity.0,
                                            transform.velocity.1
                                                + ((((transform.position.1 < target.1.position.1)
                                                    as i32
                                                    as f32)
                                                    - ((transform.position.1 > target.1.position.1)
                                                        as i32
                                                        as f32))
                                                    * (60.0 * target_distance.sqrt()
                                                        / screen_width()))
                                                .clamp(-0.25, 0.25),
                                        )
                                }
                            }
                        }

                        // Porbatabled.
                        particles.create_particle(
                            1,
                            transform.position,
                            (0.0, 0.0),
                            16.0,
                            BLACK,
                            0.5,
                            (0.0, 0.0),
                            (0.2, 0.2),
                            0.0,
                            0.0,
                        );
                    }
                    world.spawn_batch(spawn_queue);
                }

                // Bullet stuff.
                {
                    let mut bullet_has_collided: Vec<&Entity> = Vec::new();
                    let bullets: Vec<(Entity, Transform, Bullet)> = world
                        .query::<(&Transform, &Bullet)>()
                        .iter()
                        .map(|(e, (&i, &b))| (e, i, b)) // Copy out of the world
                        .collect::<Vec<_>>();
                    for bullet in &bullets {
                        for (_id, (transform, ball)) in
                            world.query_mut::<(&mut Transform, &mut Ball)>()
                        {
                            if square_distance(
                                bullet.1.position.0,
                                bullet.1.position.1,
                                transform.position.0,
                                transform.position.1,
                            ) < ball.radius.powf(2.0)
                            {
                                transform.velocity = (
                                    (transform.position.0 - bullet.1.position.0) / 2.0
                                        + (bullet.1.velocity.0 * 0.25),
                                    (transform.position.1 - bullet.1.position.1) / 2.0
                                        + (bullet.1.velocity.1 * 0.25),
                                );
                                let magnitude = (transform.velocity.0.powf(2.0)
                                    + transform.velocity.1.powf(2.0))
                                .sqrt();
                                transform.velocity = (
                                    (transform.velocity.0 / magnitude) * ball.speed,
                                    (transform.velocity.1 / magnitude) * ball.speed,
                                );
                                particles.create_particle(
                                    3,
                                    bullet.1.position,
                                    (transform.velocity.0 * 2.0, transform.velocity.1 * 2.0),
                                    8.0,
                                    WHITE,
                                    0.3,
                                    (0.1, 0.1),
                                    (4.0, 8.0),
                                    0.50,
                                    0.25,
                                );
                                bullet_has_collided.push(&bullet.0);
                                play_audio!(
                                    sink_sfx,
                                    "assets/sfx/ball_hit_side.wav",
                                    0.05,
                                    rand::RandomRange::gen_range(0.8, 1.0)
                                );
                            }
                        }
                        for (_id, (transform, bounds)) in
                            world.query_mut::<(&mut Transform, &mut Bounds)>()
                        {
                            if test_sphere_capsule(
                                (
                                    &bullet.1,
                                    &Ball {
                                        radius: bullet.2.radius,
                                        speed: 0.0,
                                    },
                                ),
                                (transform, bounds),
                            ) {
                                bounds.1 -= 1.0;
                                particles.create_particle(
                                    3,
                                    bullet.1.position,
                                    (transform.velocity.0 * 2.0, transform.velocity.1 * 2.0),
                                    8.0,
                                    WHITE,
                                    0.3,
                                    (0.1, 0.1),
                                    (4.0, 8.0),
                                    0.50,
                                    0.25,
                                );
                                bullet_has_collided.push(&bullet.0);
                                play_audio!(
                                    sink_sfx,
                                    "assets/sfx/bullet_hit_paddle.wav",
                                    0.05,
                                    rand::RandomRange::gen_range(0.8, 1.0)
                                );
                            }
                        }
                    }
                    for scrap in bullet_has_collided {
                        world.despawn(*scrap).unwrap();
                        game_state.hitstun += 1;
                    }
                }

                // Checking balls.
                {
                    let entities: Vec<(Entity, Transform, Bounds)> = world
                        .query::<(&Transform, &Bounds)>()
                        .iter()
                        .map(|(e, (&i, &b))| (e, i, b)) // Copy out of the world
                        .collect::<Vec<_>>();
                    game_state.intensity = 0.0; // Resetting the intensity.
                    for (_id, (transform, ball)) in world.query_mut::<(&mut Transform, &mut Ball)>()
                    {
                        // Doing the simple collision checks.
                        if transform.position.0 > screen_width()
                            && game_state.phase == Phase::Ongoing
                        {
                            game_state.phase = Phase::LeftWin;
                            game_state.left_score += 1;
                            particles.create_particle(
                                100,
                                transform.position,
                                (-transform.velocity.0, -transform.velocity.1),
                                4.0 * (transform.velocity.0.abs() + transform.velocity.1.abs()),
                                RED,
                                3.0,
                                (0.1, 0.1),
                                (
                                    2.0 + transform.velocity.0.abs(),
                                    8.0 + transform.velocity.0.abs(),
                                ),
                                1.0 * transform.velocity.0.abs(),
                                1.0,
                            );
                            play_audio!(sink_sfx, "assets/sfx/ball_goal.wav", 1.0, 1.0);
                            world.despawn(_id).unwrap();
                            break;
                        }
                        if transform.position.0 < 0.0 && game_state.phase == Phase::Ongoing {
                            game_state.phase = Phase::RightWin;
                            game_state.right_score += 1;
                            particles.create_particle(
                                100,
                                transform.position,
                                (-transform.velocity.0, -transform.velocity.1),
                                4.0 * (transform.velocity.0.abs() + transform.velocity.1.abs()),
                                BLUE,
                                3.0,
                                (0.1, 0.1),
                                (
                                    2.0 + transform.velocity.0.abs(),
                                    8.0 + transform.velocity.0.abs(),
                                ),
                                1.0 * transform.velocity.0.abs(),
                                1.0,
                            );
                            play_audio!(sink_sfx, "assets/sfx/ball_goal.wav", 1.0, 1.0);
                            world.despawn(_id).unwrap();
                            break;
                        }
                        if transform.position.1 < 0.0 || transform.position.1 > screen_height() {
                            transform.velocity.1 = transform.velocity.1 * -1.0;
                            transform.position = (
                                transform.position.0,
                                transform.position.1.clamp(0.0, screen_height()),
                            );
                            play_audio!(
                                sink_sfx,
                                "assets/sfx/ball_hit_side.wav",
                                0.1,
                                rand::RandomRange::gen_range(0.8, 1.0)
                            );
                        }

                        // Now checking against paddles.
                        for (_id, paddle_transform, bounds) in &entities {
                            if test_sphere_capsule((transform, ball), (paddle_transform, bounds)) {
                                ball.speed = ball.speed + (0.5 / ball.speed);
                                transform.velocity = (
                                    (transform.position.0 - paddle_transform.position.0) / bounds.0
                                        + (paddle_transform.velocity.0 * 0.25),
                                    (transform.position.1 - paddle_transform.position.1) / bounds.1
                                        + (paddle_transform.velocity.1 * 0.25),
                                );
                                let magnitude = (transform.velocity.0.powf(2.0)
                                    + transform.velocity.1.powf(2.0))
                                .sqrt();
                                transform.velocity = (
                                    (transform.velocity.0 / magnitude) * ball.speed,
                                    (transform.velocity.1 / magnitude) * ball.speed,
                                );
                                particles.create_particle(
                                    transform.velocity.0.abs() as i32,
                                    transform.position,
                                    (transform.velocity.0 * 2.0, transform.velocity.1 * 2.0),
                                    4.0 * transform.velocity.0.abs(),
                                    WHITE,
                                    0.3,
                                    (0.1, 0.1),
                                    (
                                        2.0 + transform.velocity.0.abs(),
                                        4.0 + transform.velocity.0.abs(),
                                    ),
                                    0.25 * transform.velocity.0.abs(),
                                    0.25,
                                );
                                play_audio!(
                                    sink_sfx,
                                    "assets/sfx/ball_hit_paddle.wav",
                                    0.15,
                                    rand::RandomRange::gen_range(0.8, 1.0)
                                );
                                game_state.hitstun += (ball.speed * 2.0) as i32;
                            }
                        }

                        // And updating our values.
                        game_state.intensity += ball.speed;

                        // Oh and our particles.
                        particles.create_particle(
                            1,
                            transform.position,
                            (0.0, 0.0),
                            16.0,
                            BLACK,
                            (game_state.intensity / 4.0) as f64,
                            (0.0, 0.0),
                            (0.2, 0.2),
                            0.0,
                            0.0,
                        );
                    }
                    game_state.intensity *= 4.0;
                }
            }
        } else {
            game_state.hitstun -= 1;
        }

        if frame_count % 8 == 0 {
            particles.create_particle(
                1,
                (screen_width() / 2.0, -4.0),
                (0.0, 0.4),
                2.0,
                WHITE,
                60.0,
                (screen_width() / 2.0, 0.0),
                (0.0, 0.2),
                0.0,
                0.0,
            );
        }

        next_frame().await
    }
}
