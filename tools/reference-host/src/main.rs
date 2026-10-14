//! macOS offline VST3 measurement host. Uses only the public VST3 interface.
//! JSON-lines commands on stdin; planar float32 PCM files in/out. No audio device.
#![allow(non_snake_case, unsafe_op_in_unsafe_fn)]
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    ffi::c_void,
    io::{self, BufRead, Cursor, Read, Seek, SeekFrom, Write},
    ptr,
};
use vst3::{
    Class, ComPtr, ComWrapper, Interface,
    Steinberg::{Vst::*, *},
};

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFURLCreateFromFileSystemRepresentation(
        a: *const c_void,
        p: *const u8,
        n: isize,
        dir: u8,
    ) -> *mut c_void;
    fn CFBundleCreate(a: *const c_void, url: *const c_void) -> *mut c_void;
    fn CFBundleLoadExecutable(bundle: *const c_void) -> u8;
    fn CFStringCreateWithCString(a: *const c_void, p: *const i8, encoding: u32) -> *mut c_void;
    fn CFBundleGetFunctionPointerForName(bundle: *const c_void, name: *const c_void)
    -> *mut c_void;
    fn CFRelease(obj: *const c_void);
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after: u8) -> i32;
}
#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {
    fn NSApplicationLoad() -> bool;
}

fn ok(code: tresult, operation: &str) -> Result<(), String> {
    if code == kResultOk {
        Ok(())
    } else {
        Err(format!("{operation}: VST3 result {code}"))
    }
}
fn utf16(text: &[u16]) -> String {
    String::from_utf16_lossy(&text[..text.iter().position(|x| *x == 0).unwrap_or(text.len())])
}
fn field<'a>(v: &'a Value, key: &str) -> Result<&'a str, String> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("Missing string {key}"))
}

struct Host;
impl Class for Host {
    type Interfaces = (IHostApplication, IComponentHandler);
}
impl IHostApplicationTrait for Host {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        *name = [0; 128];
        for (dst, src) in (*name)
            .iter_mut()
            .zip("Lullaby Reference Capture".encode_utf16())
        {
            *dst = src;
        }
        kResultOk
    }
    unsafe fn createInstance(
        &self,
        cid: *mut TUID,
        iid: *mut TUID,
        out: *mut *mut c_void,
    ) -> tresult {
        if out.is_null() || cid.is_null() || iid.is_null() {
            return kInvalidArgument;
        }
        *out = ptr::null_mut();
        let matches = |id: [u8; 16]| {
            std::slice::from_raw_parts(cid.cast::<u8>(), 16) == id
                && std::slice::from_raw_parts(iid.cast::<u8>(), 16) == id
        };
        if matches(IMessage::IID) {
            let attributes = ComWrapper::new(Attributes::default())
                .to_com_ptr::<IAttributeList>()
                .unwrap();
            *out = ComWrapper::new(Message {
                id: RefCell::new(std::ffi::CString::default()),
                attributes,
            })
            .to_com_ptr::<IMessage>()
            .unwrap()
            .into_raw()
            .cast();
            kResultOk
        } else if matches(IAttributeList::IID) {
            *out = ComWrapper::new(Attributes::default())
                .to_com_ptr::<IAttributeList>()
                .unwrap()
                .into_raw()
                .cast();
            kResultOk
        } else {
            kNotImplemented
        }
    }
}

#[derive(Default)]
struct Attributes(RefCell<std::collections::BTreeMap<String, Vec<u8>>>);
impl Class for Attributes {
    type Interfaces = (IAttributeList,);
}
unsafe fn attr_key(id: *const i8) -> String {
    std::ffi::CStr::from_ptr(id).to_string_lossy().into_owned()
}
impl IAttributeListTrait for Attributes {
    unsafe fn setInt(&self, id: *const i8, value: i64) -> tresult {
        self.0
            .borrow_mut()
            .insert(attr_key(id), value.to_ne_bytes().to_vec());
        kResultOk
    }
    unsafe fn getInt(&self, id: *const i8, value: *mut i64) -> tresult {
        if let Some(v) = self.0.borrow().get(&attr_key(id)).filter(|v| v.len() == 8) {
            *value = i64::from_ne_bytes(v[..].try_into().unwrap());
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn setFloat(&self, id: *const i8, value: f64) -> tresult {
        self.0
            .borrow_mut()
            .insert(attr_key(id), value.to_ne_bytes().to_vec());
        kResultOk
    }
    unsafe fn getFloat(&self, id: *const i8, value: *mut f64) -> tresult {
        if let Some(v) = self.0.borrow().get(&attr_key(id)).filter(|v| v.len() == 8) {
            *value = f64::from_ne_bytes(v[..].try_into().unwrap());
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn setString(&self, id: *const i8, text: *const u16) -> tresult {
        let mut n = 0;
        while *text.add(n) != 0 {
            n += 1;
        }
        self.0.borrow_mut().insert(
            attr_key(id),
            std::slice::from_raw_parts(text.cast::<u8>(), (n + 1) * 2).to_vec(),
        );
        kResultOk
    }
    unsafe fn getString(&self, id: *const i8, text: *mut u16, size: u32) -> tresult {
        if let Some(v) = self.0.borrow().get(&attr_key(id)) {
            if size < 2 {
                return kInvalidArgument;
            }
            let n = v.len().min(size as usize - 2);
            ptr::copy_nonoverlapping(v.as_ptr(), text.cast::<u8>(), n);
            *text.add(n / 2) = 0;
            kResultOk
        } else {
            kResultFalse
        }
    }
    unsafe fn setBinary(&self, id: *const i8, data: *const c_void, size: u32) -> tresult {
        self.0.borrow_mut().insert(
            attr_key(id),
            std::slice::from_raw_parts(data.cast::<u8>(), size as usize).to_vec(),
        );
        kResultOk
    }
    unsafe fn getBinary(&self, id: *const i8, data: *mut *const c_void, size: *mut u32) -> tresult {
        if let Some(v) = self.0.borrow().get(&attr_key(id)) {
            *data = v.as_ptr().cast();
            *size = v.len() as u32;
            kResultOk
        } else {
            kResultFalse
        }
    }
}
struct Message {
    id: RefCell<std::ffi::CString>,
    attributes: ComPtr<IAttributeList>,
}
impl Class for Message {
    type Interfaces = (IMessage,);
}
impl IMessageTrait for Message {
    unsafe fn getMessageID(&self) -> *const i8 {
        self.id.borrow().as_ptr()
    }
    unsafe fn setMessageID(&self, id: *const i8) {
        *self.id.borrow_mut() = std::ffi::CStr::from_ptr(id).to_owned();
    }
    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        self.attributes.as_ptr()
    }
}
impl IComponentHandlerTrait for Host {
    unsafe fn beginEdit(&self, _: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, _: ParamID, _: ParamValue) -> tresult {
        kResultOk
    }
    unsafe fn endEdit(&self, _: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, _: i32) -> tresult {
        kResultOk
    }
}

struct Stream(RefCell<Cursor<Vec<u8>>>);
impl Class for Stream {
    type Interfaces = (IBStream,);
}
impl IBStreamTrait for Stream {
    unsafe fn read(&self, data: *mut c_void, n: i32, count: *mut i32) -> tresult {
        if n < 0 || data.is_null() {
            return kInvalidArgument;
        }
        match self
            .0
            .borrow_mut()
            .read(std::slice::from_raw_parts_mut(data.cast(), n as usize))
        {
            Ok(read) => {
                if !count.is_null() {
                    *count = read as i32;
                }
                if read > 0 { kResultOk } else { kResultFalse }
            }
            Err(_) => kResultFalse,
        }
    }
    unsafe fn write(&self, data: *mut c_void, n: i32, count: *mut i32) -> tresult {
        if n < 0 || data.is_null() {
            return kInvalidArgument;
        }
        match self
            .0
            .borrow_mut()
            .write_all(std::slice::from_raw_parts(data.cast(), n as usize))
        {
            Ok(()) => {
                if !count.is_null() {
                    *count = n;
                }
                kResultOk
            }
            Err(_) => kResultFalse,
        }
    }
    unsafe fn seek(&self, pos: i64, mode: i32, result: *mut i64) -> tresult {
        let from = match mode {
            0 if pos >= 0 => SeekFrom::Start(pos as u64),
            1 => SeekFrom::Current(pos),
            2 => SeekFrom::End(pos),
            _ => return kInvalidArgument,
        };
        match self.0.borrow_mut().seek(from) {
            Ok(p) => {
                if !result.is_null() {
                    *result = p as i64;
                }
                kResultOk
            }
            Err(_) => kResultFalse,
        }
    }
    unsafe fn tell(&self, pos: *mut i64) -> tresult {
        if pos.is_null() {
            return kInvalidArgument;
        }
        *pos = self.0.borrow().position() as i64;
        kResultOk
    }
}

struct Queue {
    id: ParamID,
    points: Vec<(i32, f64)>,
}
impl Class for Queue {
    type Interfaces = (IParamValueQueue,);
}
impl IParamValueQueueTrait for Queue {
    unsafe fn getParameterId(&self) -> ParamID {
        self.id
    }
    unsafe fn getPointCount(&self) -> i32 {
        self.points.len() as i32
    }
    unsafe fn getPoint(&self, index: i32, offset: *mut i32, value: *mut f64) -> tresult {
        if let Some((o, v)) = self.points.get(index as usize) {
            *offset = *o;
            *value = *v;
            kResultOk
        } else {
            kInvalidArgument
        }
    }
    unsafe fn addPoint(&self, _: i32, _: f64, _: *mut i32) -> tresult {
        kNotImplemented
    }
}
struct Changes(Vec<ComPtr<IParamValueQueue>>);
impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}
impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> i32 {
        self.0.len() as i32
    }
    unsafe fn getParameterData(&self, index: i32) -> *mut IParamValueQueue {
        self.0
            .get(index as usize)
            .map_or(ptr::null_mut(), |p| p.as_ptr())
    }
    unsafe fn addParameterData(&self, _: *const ParamID, _: *mut i32) -> *mut IParamValueQueue {
        ptr::null_mut()
    }
}

struct Plugin {
    component: ComPtr<IComponent>,
    controller: ComPtr<IEditController>,
    processor: ComPtr<IAudioProcessor>,
    separate: bool,
    parameters: Vec<ParameterInfo>,
    _host: ComWrapper<Host>,
    active: bool,
}
impl Plugin {
    unsafe fn new(factory: &ComPtr<IPluginFactory>, cid: &TUID) -> Result<Self, String> {
        let host = ComWrapper::new(Host);
        let context = host.to_com_ptr::<IHostApplication>().unwrap();
        let handler = host.to_com_ptr::<IComponentHandler>().unwrap();
        let mut raw = ptr::null_mut();
        ok(
            factory.createInstance(cid.as_ptr(), IComponent::IID.as_ptr().cast(), &mut raw),
            "create component",
        )?;
        let component = ComPtr::<IComponent>::from_raw(raw.cast()).ok_or("Null component")?;
        ok(
            component.initialize(context.as_ptr().cast()),
            "initialize component",
        )?;
        let (controller, separate) = if let Some(controller) = component.cast::<IEditController>() {
            (controller, false)
        } else {
            let mut controller_id: TUID = [0; 16];
            ok(
                component.getControllerClassId(&mut controller_id),
                "controller class",
            )?;
            ok(
                factory.createInstance(
                    controller_id.as_ptr(),
                    IEditController::IID.as_ptr().cast(),
                    &mut raw,
                ),
                "create controller",
            )?;
            let controller =
                ComPtr::<IEditController>::from_raw(raw.cast()).ok_or("Null controller")?;
            ok(
                controller.initialize(context.as_ptr().cast()),
                "initialize controller",
            )?;
            (controller, true)
        };
        ok(
            controller.setComponentHandler(handler.as_ptr()),
            "set handler",
        )?;
        if separate {
            if let (Some(a), Some(b)) = (
                component.cast::<IConnectionPoint>(),
                controller.cast::<IConnectionPoint>(),
            ) {
                ok(a.connect(b.as_ptr()), "connect component")?;
                ok(b.connect(a.as_ptr()), "connect controller")?;
            }
            let stream = ComWrapper::new(Stream(RefCell::new(Cursor::new(Vec::new()))));
            let s = stream.to_com_ptr::<IBStream>().unwrap();
            ok(component.getState(s.as_ptr()), "initial state")?;
            s.seek(0, 0, ptr::null_mut());
            ok(
                controller.setComponentState(s.as_ptr()),
                "initial controller state",
            )?;
        }
        let processor = component
            .cast::<IAudioProcessor>()
            .ok_or("No audio processor")?;
        let mut parameters = Vec::new();
        for index in 0..controller.getParameterCount() {
            let mut info = std::mem::zeroed();
            ok(
                controller.getParameterInfo(index, &mut info),
                "parameter info",
            )?;
            parameters.push(info);
        }
        Ok(Self {
            component,
            controller,
            processor,
            separate,
            parameters,
            _host: host,
            active: false,
        })
    }
    unsafe fn text(&self, id: ParamID, value: f64) -> String {
        let mut text = [0; 128];
        if self.controller.getParamStringByValue(id, value, &mut text) == kResultOk {
            utf16(&text)
        } else {
            String::new()
        }
    }
    fn param(&self, name: &str) -> Result<&ParameterInfo, String> {
        // Nested parameter groups may repeat their short titles (e.g. eight
        // bands named "Enabled"). Select those by their stable VST3 ID.
        if let Some(id) = name.strip_prefix('#') {
            let id: ParamID = id
                .parse()
                .map_err(|_| format!("Invalid parameter ID {name}"))?;
            return self
                .parameters
                .iter()
                .find(|p| p.id == id)
                .ok_or_else(|| format!("Unknown parameter ID {name}"));
        }
        let mut matches = self.parameters.iter().filter(|p| utf16(&p.title) == name);
        let parameter = matches
            .next()
            .ok_or_else(|| format!("Unknown parameter {name}"))?;
        if matches.next().is_some() {
            return Err(format!("Ambiguous parameter {name}; use #<VST3 ID>"));
        }
        Ok(parameter)
    }
    unsafe fn value(&self, name: &str, value: &Value) -> Result<(ParamID, f64), String> {
        let info = self.param(name)?;
        let id = info.id;
        if let Some(raw) = value["raw"].as_f64() {
            if !(0.0..=1.0).contains(&raw) {
                return Err(format!("Invalid normalized value for {name}"));
            }
            return Ok((id, raw));
        }
        let text = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
        if info.stepCount > 0 {
            for step in 0..=info.stepCount {
                let raw = step as f64 / info.stepCount as f64;
                if self.text(id, raw).eq_ignore_ascii_case(&text) {
                    return Ok((id, raw));
                }
            }
            return Err(format!("Unknown enum value {name}={text}"));
        }
        if text.parse::<f64>().is_err() {
            for step in 0..=1000 {
                let raw = step as f64 / 1000.;
                if self.text(id, raw).eq_ignore_ascii_case(&text) {
                    return Ok((id, raw));
                }
            }
        }
        let mut input = text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut raw = 0.;
        ok(
            self.controller
                .getParamValueByString(id, input.as_mut_ptr(), &mut raw),
            &format!("parse {name}={text}"),
        )?;
        if !(0.0..=1.0).contains(&raw) {
            return Err(format!("Out of range {name}={text}"));
        }
        Ok((id, raw))
    }
    unsafe fn snapshot(&self, grid: bool) -> Value {
        Value::Array(self.parameters.iter().map(|p| {
            let raw = self.controller.getParamNormalized(p.id);
            let mut row = json!({"id":p.id,"name":utf16(&p.title),"units":utf16(&p.units),"step_count":p.stepCount,
                "flags":p.flags,"default_normalized":p.defaultNormalizedValue,"normalized":raw,"display":self.text(p.id,raw),
                "plain":self.controller.normalizedParamToPlain(p.id,raw)});
            if grid {
                let steps = if p.stepCount > 0 && p.stepCount < 1000 { p.stepCount } else { 1000 };
                row["mapping"] = Value::Array((0..=steps).map(|i| { let r=i as f64/steps as f64;
                    json!({"normalized":r,"display":self.text(p.id,r),"plain":self.controller.normalizedParamToPlain(p.id,r)}) }).collect());
            } row
        }).collect())
    }
    unsafe fn save_state(&self, path: &str) -> Result<(), String> {
        let stream = ComWrapper::new(Stream(RefCell::new(Cursor::new(Vec::new()))));
        let s = stream.to_com_ptr::<IBStream>().unwrap();
        ok(self.component.getState(s.as_ptr()), "save processor state")?;
        std::fs::write(path, stream.0.borrow().get_ref()).map_err(|e| e.to_string())
    }
    unsafe fn render(&mut self, job: &Value) -> Result<Value, String> {
        if let Some(path) = job["state_input"].as_str() {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            let stream = ComWrapper::new(Stream(RefCell::new(Cursor::new(bytes))));
            let s = stream.to_com_ptr::<IBStream>().unwrap();
            ok(self.component.setState(s.as_ptr()), "load preset state")?;
            s.seek(0, 0, ptr::null_mut());
            ok(
                self.controller.setComponentState(s.as_ptr()),
                "load controller preset state",
            )?;
        }
        let channels = job["channels"].as_u64().unwrap_or(2) as usize;
        let frames = job["frames"].as_u64().ok_or("Missing frames")? as usize;
        let rate = job["sample_rate"].as_f64().ok_or("Missing sample rate")?;
        let sizes: Vec<usize> = job["block_sizes"]
            .as_array()
            .ok_or("Missing block sizes")?
            .iter()
            .map(|x| x.as_u64().unwrap_or(0) as usize)
            .collect();
        let max_block = *sizes.iter().max().ok_or("Empty block sizes")?;
        if sizes.contains(&0) || max_block > 65536 || channels == 0 || channels > 16 {
            return Err("Invalid layout/block size".into());
        }
        let arrangement = job["arrangement"].as_u64().unwrap_or(if channels == 1 {
            SpeakerArr::kMono
        } else {
            SpeakerArr::kStereo
        });
        if arrangement.count_ones() as usize != channels {
            return Err("Arrangement/channel mismatch".into());
        }
        let input_buses = self.component.getBusCount(0, 0);
        let sidechain = job.get("sidechain_input").and_then(Value::as_str);
        if sidechain.is_some() && input_buses < 2 {
            return Err("No sidechain bus".into());
        }
        let mut ins = vec![arrangement];
        if input_buses > 1 {
            ins.push(if channels == 1 {
                SpeakerArr::kMono
            } else {
                SpeakerArr::kStereo
            });
        }
        let mut outs = [arrangement];
        ok(
            self.processor.setBusArrangements(
                ins.as_mut_ptr(),
                ins.len() as i32,
                outs.as_mut_ptr(),
                1,
            ),
            "bus arrangement",
        )?;
        ok(self.component.activateBus(0, 0, 0, 1), "activate input")?;
        ok(self.component.activateBus(0, 1, 0, 1), "activate output")?;
        if input_buses > 1 {
            ok(
                self.component
                    .activateBus(0, 0, 1, sidechain.is_some() as u8),
                "activate sidechain",
            )?;
        }
        let mut setup = ProcessSetup {
            processMode: job["process_mode"].as_i64().unwrap_or(0) as i32,
            symbolicSampleSize: 0,
            maxSamplesPerBlock: max_block as i32,
            sampleRate: rate,
        };
        ok(
            self.processor.setupProcessing(&mut setup),
            "setup processing",
        )?;
        let mut initial = Vec::new();
        if let Some(settings) = job["settings"].as_object() {
            for (name, value) in settings {
                let (id, raw) = self.value(name, value)?;
                ok(
                    self.controller.setParamNormalized(id, raw),
                    &format!("set {name}"),
                )?;
                initial.push((id, raw));
            }
        }
        CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.02, 0);
        // Re-run setup after latency/quality changes, before activation.
        ok(
            self.processor.setupProcessing(&mut setup),
            "setup configured processing",
        )?;
        ok(self.component.setActive(1), "activate")?;
        self.active = true;
        ok(self.processor.setProcessing(1), "start processing")?;
        let read_pcm = |path: &str, ch: usize| -> Result<Vec<Vec<f32>>, String> {
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            if bytes.len() != frames * ch * 4 {
                return Err(format!("Wrong input size {path}"));
            }
            Ok(bytes
                .chunks_exact(frames * 4)
                .map(|c| {
                    c.chunks_exact(4)
                        .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                        .collect()
                })
                .collect())
        };
        let input = read_pcm(field(job, "input")?, channels)?;
        let key = if let Some(path) = sidechain {
            read_pcm(path, 2)?
        } else {
            vec![vec![0.; frames]; 2]
        };
        let events = job["automation"].as_array().cloned().unwrap_or_default();
        let events: Vec<(usize, ParamID, f64)> = events
            .iter()
            .map(|e| {
                let (id, r) = self.value(field(e, "parameter")?, &e["value"])?;
                Ok((
                    e["frame"].as_u64().ok_or("Missing event frame")? as usize,
                    id,
                    r,
                ))
            })
            .collect::<Result<_, String>>()?;
        let mut in_buffers = vec![vec![0.; max_block]; channels];
        let mut key_buffers = vec![vec![0.; max_block]; 2];
        let mut out_buffers = vec![vec![0.; max_block]; channels];
        let mut in_ptrs: Vec<_> = in_buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
        let mut key_ptrs: Vec<_> = key_buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
        let mut out_ptrs: Vec<_> = out_buffers.iter_mut().map(|b| b.as_mut_ptr()).collect();
        let bus = |p, n| AudioBusBuffers {
            numChannels: n,
            silenceFlags: 0,
            __field0: AudioBusBuffers__type0 {
                channelBuffers32: p,
            },
        };
        let mut inputs = vec![bus(in_ptrs.as_mut_ptr(), channels as i32)];
        if input_buses > 1 {
            inputs.push(bus(
                key_ptrs.as_mut_ptr(),
                if channels == 1 { 1 } else { 2 },
            ));
        }
        let mut outputs = [bus(out_ptrs.as_mut_ptr(), channels as i32)];
        let mut output = vec![Vec::<f32>::with_capacity(frames); channels];
        let mut offset = 0;
        let mut block = 0;
        let mut latency = 0;
        let mut latency_changes = Vec::new();
        while offset < frames {
            let count = sizes[block % sizes.len()].min(frames - offset);
            for c in 0..channels {
                in_buffers[c][..count].copy_from_slice(&input[c][offset..offset + count]);
            }
            for c in 0..2 {
                key_buffers[c][..count].copy_from_slice(&key[c][offset..offset + count]);
            }
            let mut values = std::collections::BTreeMap::<ParamID, Vec<(i32, f64)>>::new();
            if offset == 0 {
                for (id, r) in &initial {
                    values.entry(*id).or_default().push((0, *r));
                }
            }
            for (frame, id, r) in &events {
                if *frame >= offset && *frame < offset + count {
                    values
                        .entry(*id)
                        .or_default()
                        .push(((*frame - offset) as i32, *r));
                    ok(
                        self.controller.setParamNormalized(*id, *r),
                        "automation controller",
                    )?;
                }
            }
            let queues = values
                .into_iter()
                .map(|(id, points)| {
                    ComWrapper::new(Queue { id, points })
                        .to_com_ptr::<IParamValueQueue>()
                        .unwrap()
                })
                .collect();
            let changes = ComWrapper::new(Changes(queues))
                .to_com_ptr::<IParameterChanges>()
                .unwrap();
            let mut context: ProcessContext = std::mem::zeroed();
            context.sampleRate = rate;
            context.projectTimeSamples = offset as i64;
            context.tempo = 120.;
            context.timeSigNumerator = 4;
            context.timeSigDenominator = 4;
            context.state = ProcessContext_::StatesAndFlags_::kPlaying
                | ProcessContext_::StatesAndFlags_::kTempoValid
                | ProcessContext_::StatesAndFlags_::kTimeSigValid;
            let mut data = ProcessData {
                processMode: setup.processMode,
                symbolicSampleSize: 0,
                numSamples: count as i32,
                numInputs: inputs.len() as i32,
                numOutputs: 1,
                inputs: inputs.as_mut_ptr(),
                outputs: outputs.as_mut_ptr(),
                inputParameterChanges: changes.as_ptr(),
                outputParameterChanges: ptr::null_mut(),
                inputEvents: ptr::null_mut(),
                outputEvents: ptr::null_mut(),
                processContext: &mut context,
            };
            ok(self.processor.process(&mut data), "process")?;
            let current = self.processor.getLatencySamples();
            if offset == 0 || current != latency {
                latency_changes.push(json!({"frame":offset,"samples":current}));
                latency = current;
            }
            for c in 0..channels {
                if out_buffers[c][..count].iter().any(|x| !x.is_finite()) {
                    return Err("Non-finite output".into());
                }
                output[c].extend_from_slice(&out_buffers[c][..count]);
            }
            offset += count;
            block += 1;
        }
        let mut bytes = Vec::with_capacity(frames * channels * 4);
        for channel in &output {
            for x in channel {
                bytes.extend_from_slice(&x.to_le_bytes());
            }
        }
        std::fs::write(field(job, "output")?, bytes).map_err(|e| e.to_string())?;
        if let Some(path) = job["state_output"].as_str() {
            self.save_state(path)?;
        }
        // The processor's serialized state is authoritative after audio-thread changes.
        let stream = ComWrapper::new(Stream(RefCell::new(Cursor::new(Vec::new()))));
        let s = stream.to_com_ptr::<IBStream>().unwrap();
        ok(self.component.getState(s.as_ptr()), "read rendered state")?;
        s.seek(0, 0, ptr::null_mut());
        ok(
            self.controller.setComponentState(s.as_ptr()),
            "sync rendered controller",
        )?;
        let parameters = self.snapshot(false);
        ok(self.processor.setProcessing(0), "stop processing")?;
        ok(self.component.setActive(0), "deactivate")?;
        self.active = false;
        Ok(
            json!({"latency_samples":latency,"latency_changes":latency_changes,"parameters":parameters,"frames":frames,"channels":channels,"input_buses":input_buses,"arrangement":arrangement,"process_mode":setup.processMode,"blocks":block}),
        )
    }
}
impl Drop for Plugin {
    fn drop(&mut self) {
        unsafe {
            if self.active {
                self.processor.setProcessing(0);
                self.component.setActive(0);
            }
            if self.separate {
                if let (Some(a), Some(b)) = (
                    self.component.cast::<IConnectionPoint>(),
                    self.controller.cast::<IConnectionPoint>(),
                ) {
                    a.disconnect(b.as_ptr());
                    b.disconnect(a.as_ptr());
                }
                self.controller.setComponentHandler(ptr::null_mut());
                self.controller.terminate();
            }
            self.component.terminate();
        }
    }
}

unsafe fn main_inner() -> Result<(), String> {
    NSApplicationLoad();
    let path = std::env::args()
        .nth(1)
        .ok_or("Usage: reference-host /absolute/path/plugin.vst3")?;
    let url =
        CFURLCreateFromFileSystemRepresentation(ptr::null(), path.as_ptr(), path.len() as isize, 1);
    let bundle = CFBundleCreate(ptr::null(), url);
    CFRelease(url);
    if bundle.is_null() || CFBundleLoadExecutable(bundle) == 0 {
        return Err("Cannot load plugin bundle".into());
    }
    let symbol = |name: &str| {
        let c = std::ffi::CString::new(name).unwrap();
        let s = CFStringCreateWithCString(ptr::null(), c.as_ptr(), 0x08000100);
        let p = CFBundleGetFunctionPointerForName(bundle, s);
        CFRelease(s);
        p
    };
    let entry = symbol("bundleEntry");
    if !entry.is_null()
        && !std::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void) -> bool>(entry)(
            bundle,
        )
    {
        return Err("bundleEntry failed".into());
    }
    let get_factory = symbol("GetPluginFactory");
    if get_factory.is_null() {
        return Err("Missing GetPluginFactory".into());
    }
    let factory = ComPtr::<IPluginFactory>::from_raw(std::mem::transmute::<
        *mut c_void,
        unsafe extern "C" fn() -> *mut IPluginFactory,
    >(get_factory)())
    .ok_or("Null factory")?;
    let mut class = std::mem::zeroed::<PClassInfo>();
    let mut found = false;
    for index in 0..factory.countClasses() {
        ok(factory.getClassInfo(index, &mut class), "class info")?;
        let category = std::ffi::CStr::from_ptr(class.category.as_ptr()).to_string_lossy();
        if category == "Audio Module Class" {
            found = true;
            break;
        }
    }
    if !found {
        return Err("No audio class".into());
    }
    for line in io::stdin().lock().lines() {
        let result = (|| -> Result<Value, String> {
            let job: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            let mut p = Plugin::new(&factory, &class.cid)?;
            if job["command"] == "inspect" {
                if let Some(path) = job["state_output"].as_str() {
                    p.save_state(path)?;
                }
                Ok(
                    json!({"parameters":p.snapshot(true),"input_buses":p.component.getBusCount(0,0),"output_buses":p.component.getBusCount(0,1)}),
                )
            } else {
                p.render(&job)
            }
        })();
        println!(
            "{}",
            match result {
                Ok(v) => json!({"ok":true,"result":v}),
                Err(e) => json!({"ok":false,"error":e}),
            }
        );
        io::stdout().flush().map_err(|e| e.to_string())?;
    }
    drop(factory);
    let exit = symbol("bundleExit");
    if !exit.is_null() {
        std::mem::transmute::<*mut c_void, unsafe extern "C" fn() -> bool>(exit)();
    }
    CFRelease(bundle);
    Ok(())
}
fn main() {
    if let Err(error) = unsafe { main_inner() } {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
