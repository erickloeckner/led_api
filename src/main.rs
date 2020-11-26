// build command:
// cargo +nightly build --release

#![feature(decl_macro,proc_macro_hygiene)]

#[macro_use] extern crate rocket;

use std::io::prelude::*;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{sync_channel, SyncSender, Receiver};
use std::thread::{self, sleep};
use std::time::{Duration, Instant};
use std::{env, fs, process};

use rand::Rng;
use rocket::request::Form;
use rocket::State;
use spidev::{Spidev, SpidevOptions, SpiModeFlags};

use serde_derive::Deserialize;

struct LedSender {
    sender: SyncSender<LedState>,
}

struct LedState {
    color1: ColorHsv,
    color2: ColorHsv,
    color3: ColorHsv,
    pattern: u8,
}

#[derive(FromForm)]
struct LedStateData {
    color1_h: f32,
    color1_s: f32,
    color1_v: f32,
    color2_h: f32,
    color2_s: f32,
    color2_v: f32,
    color3_h: f32,
    color3_s: f32,
    color3_v: f32,
    pattern: u8,
}

struct Pixel {
    r: u8,
    g: u8,
    b: u8,
}

struct ColorHsv {
    h: f32,
    s: f32,
    v: f32,
}

impl ColorHsv {
    fn set_brightness(&mut self, brightness: f32) {
        self.v = (self.v * brightness.max(0.0)).min(1.0);
    }
}

struct Sprite {
    pos: f32,
    falloff: f32,
}

struct SpriteEnvelope {
    pos: f32,
    falloff: f32,
    live: bool,
    state: SpriteState,
    attack: f32,
    sustain: f32,
    release: f32,
    timer: f32,
    level: f32,
    mix: f32,
}

impl SpriteEnvelope {
    fn new() -> SpriteEnvelope {
        SpriteEnvelope { 
            pos: 0.0, 
            falloff: 1.0, 
            live: true, 
            state: SpriteState::Attack,
            attack: 1000.0,
            sustain: 1000.0,
            release: 1000.0,
            timer: 0.0,
            level: 0.0,
            mix: 0.0,
        }
    }
    
    fn run(&mut self, dt: f32) {
        if self.live {
            match self.state {
                SpriteState::Attack => {
                    self.timer = self.timer + dt;
                    
                    //~ self.level = (dt * 0.005 + self.level).min(1.0);
                    self.level = (dt / self.attack + self.level).min(1.0);
                    
                    if self.level >= 1.0 {
                        self.state = SpriteState::Sustain;
                        self.timer = 0.0;
                    }
                }
                SpriteState::Sustain => {
                    self.timer = self.timer + dt;
                    
                    if self.timer >= self.sustain {
                        self.state = SpriteState::Release;
                        self.timer = 0.0;
                    }
                }
                SpriteState::Release => {
                    self.timer = self.timer + dt;
                    
                    //~ self.level = (self.level - (dt * 0.005)).max(0.0);
                    self.level = (self.level - (dt / self.release)).min(1.0);
                    
                    if self.level <= 0.0 {
                        self.live = false;
                    }
                }
            }
        }
    }
    
    fn reset(&mut self) {
        self.live = true;
        self.state = SpriteState::Attack;
        self.timer = 0.0;
        self.level = 0.0;
    }
}

#[derive(Deserialize)]
struct Config {
    main: Main,
    rand: Rand,
}

#[derive(Deserialize)]
struct Main {
    led_count: usize,
    brightness: f32,
}

#[derive(Deserialize)]
struct Rand {
    count: usize,
    falloff: f32,
}

enum SpriteState {
    Attack,
    Sustain,
    Release,
}

fn write_leds(spi: &mut Spidev, leds: &Vec<Pixel>, buffer: &mut Vec<u8>, offset: usize) {
    buffer.clear();
    for _i in 0..4 {
        buffer.push(0);
    }
    
    for i in offset..(offset + leds.len()) {
        let i_wrap = i % leds.len();
        buffer.push(255);
        buffer.push(leds[i_wrap].b);
        buffer.push(leds[i_wrap].g);
        buffer.push(leds[i_wrap].r);
    }
    
    for _i in 0..((leds.len() + 1) / 2) {
        buffer.push(0);
    }
    spi.write(&buffer[..]).unwrap();
}

fn hsv_2_rgb(col: &ColorHsv) -> Pixel {
    let h_wrap = col.h.rem_euclid(1.0);
    let mut out = Pixel { r: 0, g: 0, b: 0 };
    match (h_wrap * 6.0).trunc() as u8 {
        0 => {
            out.r = (col.v * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s * (1.0 - ((col.h * 6.0) - ((col.h * 6.0).trunc()))))) * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s)) * 255.0) as u8;
        }
        1 => {
            out.r = ((col.v * (1.0 - col.s * ((col.h * 6.0) - ((col.h * 6.0).trunc())))) * 255.0) as u8;
            out.g = (col.v * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s)) * 255.0) as u8;
        }
        2 => {
            out.r = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.g = (col.v * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s * (1.0 - ((col.h * 6.0) - ((col.h * 6.0).trunc()))))) * 255.0) as u8;
        }
        3 => {
            out.r = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s * ((col.h * 6.0) - ((col.h * 6.0).trunc())))) * 255.0) as u8;
            out.b = (col.v * 255.0) as u8;
        }
        4 => {
            out.r = ((col.v * (1.0 - col.s * (1.0 - ((col.h * 6.0) - ((col.h * 6.0).trunc()))))) * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.b = (col.v * 255.0) as u8;
        }
        5 => {
            out.r = (col.v * 255.0) as u8;
            out.g = ((col.v * (1.0 - col.s)) * 255.0) as u8;
            out.b = ((col.v * (1.0 - col.s * ((col.h * 6.0) - ((col.h * 6.0).trunc())))) * 255.0) as u8;
        }
        _ => (),
    }
    out
}

fn hsv_interp(col1: &ColorHsv, col2: &ColorHsv, pos: f32) -> ColorHsv {
    let h_range = col1.h - col2.h;
    let s_range = col1.s - col2.s;
    let v_range = col1.v - col2.v;
    
    let h_out = col1.h - (h_range * pos.min(1.0));
    let s_out = col1.s - (s_range * pos.min(1.0));
    let v_out = col1.v - (v_range * pos.min(1.0));
    
    ColorHsv { h: h_out, s: s_out, v: v_out }
}

fn hsv_interp_3(col1: &ColorHsv, col2: &ColorHsv, col3: &ColorHsv, pos: f32) -> ColorHsv {
    if pos > 0.0 {
        let h_range = col1.h - col2.h;
        let s_range = col1.s - col2.s;
        let v_range = col1.v - col2.v;
        
        let h_out = col1.h - (h_range * pos.min(1.0));
        let s_out = col1.s - (s_range * pos.min(1.0));
        let v_out = col1.v - (v_range * pos.min(1.0));

        ColorHsv { h: h_out, s: s_out, v: v_out }
    } else if pos < 0.0 {
        let h_range = col1.h - col3.h;
        let s_range = col1.s - col3.s;
        let v_range = col1.v - col3.v;
        
        let h_out = col1.h - (h_range * (pos * -1.0).min(1.0));
        let s_out = col1.s - (s_range * (pos * -1.0).min(1.0));
        let v_out = col1.v - (v_range * (pos * -1.0).min(1.0));

        ColorHsv { h: h_out, s: s_out, v: v_out }
    } else {
        let h_out = col1.h;
        let s_out = col1.s;
        let v_out = col1.v;

        ColorHsv { h: h_out, s: s_out, v: v_out }
    }
}

fn triangle(pos: f32, phs: f32) -> f32 {
    ((pos + phs).rem_euclid(1.0) * 2.0 - 1.0).abs() * 2.0 - 1.0
}

#[get("/")]
fn index() -> &'static str {
    "foo bar"
}

#[get("/get")]
fn get(leds: State<Arc<Mutex<LedState>>>) -> String {
    let leds_data = leds.lock().unwrap();
    format!("{},{},{},{},{},{},{},{},{},{}",
        leds_data.color1.h, leds_data.color1.s, leds_data.color1.v, 
        leds_data.color2.h, leds_data.color2.s, leds_data.color2.v,
        leds_data.color3.h, leds_data.color3.s, leds_data.color3.v,
        leds_data.pattern,
    )
}

#[post("/set", data = "<data>")]
fn set(led_sender: State<LedSender>, data: Form<LedStateData>, state: State<Arc<Mutex<LedState>>>) {
    led_sender.sender.send(LedState {
        color1: ColorHsv { h: data.color1_h, s: data.color1_s, v: data.color1_v },
        color2: ColorHsv { h: data.color2_h, s: data.color2_s, v: data.color2_v },
        color3: ColorHsv { h: data.color3_h, s: data.color3_s, v: data.color3_v },
        pattern: data.pattern }).unwrap();
    let mut state_data = state.lock().unwrap();
    state_data.color1 = ColorHsv { h: data.color1_h, s: data.color1_s, v: data.color1_v };
    state_data.color2 = ColorHsv { h: data.color2_h, s: data.color2_s, v: data.color2_v };
    state_data.color3 = ColorHsv { h: data.color3_h, s: data.color3_s, v: data.color3_v };
    state_data.pattern = data.pattern;
}

fn main() {
    let config_path = env::args().nth(1).unwrap_or_else(|| {
        println!("no config file specified");
        process::exit(1);
    });
    let config_raw = fs::read_to_string(&config_path).unwrap_or_else(|err| {
        println!("error reading config: {}", err);
        process::exit(1);
    });
    let config: Config = toml::from_str(&config_raw).unwrap_or_else(|err| {
        println!("error parsing config: {}", err);
        process::exit(1);
    });
    
    let (tx, rx): (SyncSender<LedState>, Receiver<LedState>) = sync_channel(1);
    
    let led_state = Arc::new(Mutex::new(LedState {
        color1: ColorHsv { h: 0.0, s: 0.0, v: 0.0 },
        color2: ColorHsv { h: 0.0, s: 0.0, v: 0.0 },
        color3: ColorHsv { h: 0.0, s: 0.0, v: 0.0 },
        pattern: 0,
    }));
    
    thread::spawn(move || {
        let mut spidev = Spidev::open("/dev/spidev0.0").unwrap();
        let options = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(8_000_000)
            .mode(SpiModeFlags::SPI_MODE_0)
            .build();
        spidev.configure(&options).unwrap();

        let mut leds_1 = Vec::with_capacity(config.main.led_count);
        for _i in 0..config.main.led_count {
            leds_1.push(Pixel { r: 0, g: 0, b: 0 });
        }

        let mut leds_off = Vec::with_capacity(config.main.led_count);
        for _i in 0..config.main.led_count {
            leds_off.push(Pixel { r: 0, g: 0, b: 0 });
        }

        let mut buffer = Vec::with_capacity(1024);
        
        let mut offset: f32 = 0.0;
        let offset_inc = 0.0005;
        
        let loop_time: f32 = 16.6666;
        let mut pattern: u8 = 0;
        
        let mut rng = rand::thread_rng();
        
        let mut color1 = ColorHsv { h: 0.0, s: 0.0, v: 0.0 };
        let mut color2 = ColorHsv { h: 0.0, s: 0.0, v: 0.0 };
        let mut color3 = ColorHsv { h: 0.0, s: 0.0, v: 0.0 };
        
        let mut scanner = Sprite { pos: 0.0, falloff: 10.0 };
        
        let mut sprites_random = Vec::with_capacity(config.rand.count);
        for _i in 0..config.rand.count {
            let mut sprite = SpriteEnvelope::new();
            sprite.pos = rng.gen();
            sprite.falloff = config.rand.falloff;
            sprite.attack = rng.gen::<f32>() * 1000.0 + 1000.0;
            sprite.sustain = 2000.0;
            sprite.release = rng.gen::<f32>() * 1000.0 + 1000.0;
            sprites_random.push(sprite);
        }

        loop {
            let loop_start = Instant::now();
            let msg = rx.try_recv();

            match msg {
                Ok(msg) => {
                    match msg.pattern {
                        0 => {
                            write_leds(&mut spidev, &leds_off, &mut buffer, 0);
                            pattern = msg.pattern;
                        }
                        1 => {
                            color1 = msg.color1;
                            color2 = msg.color2;
                            color3 = msg.color3;
                            pattern = msg.pattern;
                            color1.set_brightness(config.main.brightness);
                            color2.set_brightness(config.main.brightness);
                            color3.set_brightness(config.main.brightness);
                        }
                        2 => {
                            color1 = msg.color1;
                            color2 = msg.color2;
                            color3 = msg.color3;
                            pattern = msg.pattern;
                            color1.set_brightness(config.main.brightness);
                            color2.set_brightness(config.main.brightness);
                            color3.set_brightness(config.main.brightness);   
                        }
                        3 => {
                            color1 = msg.color1;
                            color2 = msg.color2;
                            color3 = msg.color3;
                            pattern = msg.pattern;
                            color1.set_brightness(config.main.brightness);
                            color2.set_brightness(config.main.brightness);
                            color3.set_brightness(config.main.brightness);
                            //sprite_random.pos = rng.gen();
                            for i in sprites_random.iter_mut() {
                                i.pos = rng.gen();
                            }
                        }
                        4 => {
                            color1 = msg.color1;
                            color2 = msg.color2;
                            color3 = msg.color3;
                            pattern = msg.pattern;
                            color1.set_brightness(config.main.brightness);
                            color2.set_brightness(config.main.brightness);
                            color3.set_brightness(config.main.brightness);
                            //~ sprite_random.pos = rng.gen();
                            for i in sprites_random.iter_mut() {
                                i.pos = rng.gen();
                                i.mix = rng.gen();
                            }
                        }
                        5 => {
                            color1 = msg.color1;
                            color2 = msg.color2;
                            color3 = msg.color3;
                            pattern = msg.pattern;
                            color1.set_brightness(config.main.brightness);
                            color2.set_brightness(config.main.brightness);
                            color3.set_brightness(config.main.brightness);
                        }
                        _ => (),
                    }
                }
                Err(_) => (),
            }
            
            match pattern {
                0 => (),
                1 => {
                    for (i, led) in leds_1.iter_mut().enumerate() {
                        let pos = (i as f32) / ((config.main.led_count - 1) as f32);
                        *led = hsv_2_rgb(&hsv_interp_3(&color1, &color2, &color3, triangle(0.0 + offset, pos)));
                    }
                    write_leds(&mut spidev, &leds_1, &mut buffer, 0);
                }
                2 => {
                    scanner.pos = (triangle(offset, 0.0) + 1.0) * 0.5;
                    
                    for (i, led) in leds_1.iter_mut().enumerate() {
                        let pos = (i as f32) / ((config.main.led_count - 1) as f32);
                        let delta = (pos - scanner.pos).abs();
                        let mix = (delta + (delta * scanner.falloff)).min(1.0);
                        
                        let col_interp = hsv_interp(&color1, &color2, pos);
                        
                        *led = hsv_2_rgb(&hsv_interp(&color3, &col_interp, mix));
                    }
                    
                    write_leds(&mut spidev, &leds_1, &mut buffer, 0);
                }
                3 => {                    
                    for i in sprites_random.iter_mut() {
                        if i.live == false {
                            i.pos = rng.gen();
                            i.reset();
                            i.attack = rng.gen::<f32>() * 1000.0 + 1000.0;
                            //~ sprite.sustain = 250.0;
                            i.release = rng.gen::<f32>() * 1000.0 + 1000.0;
                        }
                        i.run(loop_time);
                    }
                    
                    for (i, led) in leds_1.iter_mut().enumerate() {
                        let pos = (i as f32) / ((config.main.led_count - 1) as f32);
                        let col_interp = hsv_interp(&color1, &color2, pos);
                        let mut mix_total = 0.0;
                        
                        for i in sprites_random.iter() {
                            let delta = (pos - i.pos).abs();
                            let mix = (i.level - (delta * i.falloff)).max(0.0);
                            mix_total = (mix_total + mix).min(1.0);
                        }
                        
                        *led = hsv_2_rgb(&hsv_interp(&col_interp, &color3, mix_total));
                    }
                    
                    write_leds(&mut spidev, &leds_1, &mut buffer, 0);
                }
                4 => {
                    for i in sprites_random.iter_mut() {
                        if i.live == false {
                            i.pos = rng.gen();
                            i.reset();
                            i.attack = rng.gen::<f32>() * 1000.0 + 1000.0;
                            //~ sprite.sustain = 250.0;
                            i.release = rng.gen::<f32>() * 1000.0 + 1000.0;
                            i.mix = rng.gen::<f32>();
                        }
                        i.run(loop_time);
                    }
                    
                    for (i, led) in leds_1.iter_mut().enumerate() {
                        let pos = (i as f32) / ((config.main.led_count - 1) as f32);
                        let mut mix_total = 0.0;
                        let mut mix_max = 0.0;
                        let mut index_max: usize = 0;
                        //~ let mut col_interp = ColorHsv { h: 0.0, s: 0.0, v: 0.0 };
                        
                        for (i, sprite) in sprites_random.iter().enumerate() {
                            let delta = (pos - sprite.pos).abs();
                            let mix = (sprite.level - (delta * sprite.falloff)).max(0.0);
                            mix_total = (mix_total + mix).min(1.0);
                            
                            //~ col_interp = hsv_interp(&color2, &color3, sprite.mix);
                            
                            if mix_total > mix_max {
                                mix_max = mix_total;
                                index_max = i;
                            }
                        }
                        
                        let col_interp = hsv_interp(&color2, &color3, sprites_random[index_max].mix);
                        
                        *led = hsv_2_rgb(&hsv_interp(&color1, &col_interp, mix_total));
                    }
                    
                    write_leds(&mut spidev, &leds_1, &mut buffer, 0);
                }
                5 => {
                    for (i, led) in leds_1.iter_mut().enumerate() {
                        let pos = (i as f32) / ((config.main.led_count - 1) as f32);
                        if pos < 0.5 {
                            *led = hsv_2_rgb(&hsv_interp(&color1, &color2, pos * 2.0));
                        } else {
                            *led = hsv_2_rgb(&hsv_interp(&color2, &color3, (pos * 2.0) - 1.0));
                        }
                    }
                    write_leds(&mut spidev, &leds_1, &mut buffer, 0);
                }
                _ => (),
            }
            
            offset = (offset + offset_inc).rem_euclid(1.0);
            
            let loop_time_adj = (loop_time - (loop_start.elapsed().as_millis() as f32)) / 1000.0;
            if (loop_start.elapsed().as_millis() as f32) < loop_time {
                sleep(Duration::from_secs_f32(loop_time_adj));
            }
        }
    });

    rocket::ignite()
        .manage(LedSender { sender: tx })
        .manage(led_state)
        .mount("/", routes![index, set, get])
        .launch();
}
