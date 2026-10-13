// Loads the built component directly: no installation or audio device required.
#import <AudioToolbox/AudioToolbox.h>
#import <Cocoa/Cocoa.h>
#include <algorithm>
#include <cmath>
#include <iostream>
#include <map>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

static void require(bool ok, const std::string &message) {
    if (!ok) throw std::runtime_error(message);
}
static void check(OSStatus status, const char *operation) {
    require(status == noErr, std::string(operation) + ": OSStatus " + std::to_string(status));
}
template <typename T>
static T property(AudioUnit au, AudioUnitPropertyID id, AudioUnitScope scope = kAudioUnitScope_Global,
                  AudioUnitElement element = 0) {
    T result{};
    UInt32 size = sizeof(result);
    const auto label = "GetProperty id=" + std::to_string(id) + " scope=" + std::to_string(scope)
        + " element=" + std::to_string(element);
    check(AudioUnitGetProperty(au, id, scope, element, &result, &size), label.c_str());
    require(size == sizeof(result), "Unexpected property size");
    return result;
}
template <typename T>
static void setProperty(AudioUnit au, AudioUnitPropertyID id, T value,
                        AudioUnitScope scope = kAudioUnitScope_Global, AudioUnitElement element = 0) {
    check(AudioUnitSetProperty(au, id, scope, element, &value, sizeof(value)), "SetProperty");
}

static float sample(int64_t frame, unsigned channel, double rate, bool sidechain) {
    if (frame < 0) return 0;
    const double time = frame / rate;
    const double frequency = sidechain ? 317.0 : 1056.0;
    return static_cast<float>((channel ? 0.17 : 0.23) * std::sin(2 * M_PI * frequency * time)
        + 0.025 * std::sin(2 * M_PI * (sidechain ? 659.0 : 733.0) * time)
        + 0.020 * std::sin(2 * M_PI * (sidechain ? 977.0 : 1423.0) * time));
}
struct Input { double rate; bool sidechain; };
static OSStatus input(void *context, AudioUnitRenderActionFlags *, const AudioTimeStamp *timestamp,
                      UInt32, UInt32 frames, AudioBufferList *buffers) {
    const auto &source = *static_cast<Input *>(context);
    for (UInt32 channel = 0; channel < buffers->mNumberBuffers; ++channel) {
        auto &buffer = buffers->mBuffers[channel];
        if (!buffer.mData || buffer.mNumberChannels != 1) return kAudio_ParamError;
        buffer.mDataByteSize = frames * sizeof(float);
        auto *data = static_cast<float *>(buffer.mData);
        for (UInt32 i = 0; i < frames; ++i)
            data[i] = sample(static_cast<int64_t>(timestamp->mSampleTime) + i, channel,
                             source.rate, source.sidechain);
    }
    return noErr;
}

static std::map<std::string, AudioUnitParameterID> parameters(AudioUnit au) {
    UInt32 size = 0;
    Boolean writable = false;
    check(AudioUnitGetPropertyInfo(au, kAudioUnitProperty_ParameterList, kAudioUnitScope_Global,
                                  0, &size, &writable), "ParameterList size");
    std::vector<AudioUnitParameterID> ids(size / sizeof(AudioUnitParameterID));
    check(AudioUnitGetProperty(au, kAudioUnitProperty_ParameterList, kAudioUnitScope_Global,
                              0, ids.data(), &size), "ParameterList");
    require(ids.size() == 73 && std::set<AudioUnitParameterID>(ids.begin(), ids.end()).size() == 73,
            "Expected 73 unique AU parameters");
    std::map<std::string, AudioUnitParameterID> result;
    for (auto id : ids) {
        auto info = property<AudioUnitParameterInfo>(au, kAudioUnitProperty_ParameterInfo,
                                                     kAudioUnitScope_Global, id);
        char name[256]{};
        if (info.flags & kAudioUnitParameterFlag_HasCFNameString) {
            require(CFStringGetCString(info.cfNameString, name, sizeof(name), kCFStringEncodingUTF8), "Parameter name");
            if (info.flags & kAudioUnitParameterFlag_CFNameRelease) CFRelease(info.cfNameString);
        } else {
            std::copy(std::begin(info.name), std::end(info.name), name);
        }
        result[name] = id;
        if (std::string(name) == "Depth") {
            require(info.minValue == 0 && info.maxValue == 1 && info.defaultValue == 0.25f,
                    "Depth must retain CLAP normalized range/default");
        }
    }
    return result;
}

static void set(AudioUnit au, AudioUnitParameterID id, float value) {
    check(AudioUnitSetParameter(au, id, kAudioUnitScope_Global, 0, value, 0), "SetParameter");
}
static float get(AudioUnit au, AudioUnitParameterID id) {
    float value = 0;
    check(AudioUnitGetParameter(au, id, kAudioUnitScope_Global, 0, &value), "GetParameter");
    return value;
}

static void render(AudioComponent component, double rate, int quality, bool listenSidechain,
                   bool suppress = false) {
    AudioUnit au = nullptr;
    check(AudioComponentInstanceNew(component, &au), "InstanceNew");
    const auto params = parameters(au);
    require(property<UInt32>(au, kAudioUnitProperty_ElementCount, kAudioUnitScope_Input) == 2,
            "Main input and sidechain must be exposed");
    require(property<UInt32>(au, kAudioUnitProperty_ElementCount, kAudioUnitScope_Output) == 1,
            "Expected one output bus");
    AudioStreamBasicDescription format{};
    format.mSampleRate = rate;
    format.mFormatID = kAudioFormatLinearPCM;
    format.mFormatFlags = kAudioFormatFlagsNativeFloatPacked | kAudioFormatFlagIsNonInterleaved;
    format.mBytesPerPacket = format.mBytesPerFrame = sizeof(float);
    format.mFramesPerPacket = 1;
    format.mChannelsPerFrame = 2;
    format.mBitsPerChannel = 32;
    for (UInt32 bus = 0; bus < 2; ++bus)
        setProperty(au, kAudioUnitProperty_StreamFormat, format, kAudioUnitScope_Input, bus);
    setProperty(au, kAudioUnitProperty_StreamFormat, format, kAudioUnitScope_Output);
    setProperty<UInt32>(au, kAudioUnitProperty_MaximumFramesPerSlice, 512);
    Input mainInput{rate, false}, sideInput{rate, true};
    setProperty(au, kAudioUnitProperty_SetRenderCallback, AURenderCallbackStruct{input, &mainInput}, kAudioUnitScope_Input);
    if (listenSidechain)
        setProperty(au, kAudioUnitProperty_SetRenderCallback, AURenderCallbackStruct{input, &sideInput}, kAudioUnitScope_Input, 1);
    set(au, params.at("Depth"), suppress ? 0.5f : 0.0f);
    set(au, params.at("Quality"), static_cast<float>(quality));
    set(au, params.at("External sidechain"), listenSidechain ? 1 : 0);
    set(au, params.at("Listen to sidechain"), listenSidechain ? 1 : 0);
    check(AudioUnitInitialize(au), "Initialize");
    const int latency = quality == 3 ? 512 : 2048;
    const double reported = property<Float64>(au, kAudioUnitProperty_Latency);
    require(std::abs(reported * rate - latency) < 0.01, "Incorrect reported AU latency");
    std::vector<float> left(512), right(512);
    struct StereoBuffers { UInt32 count; AudioBuffer buffers[2]; } storage{};
    storage.count = 2;
    const UInt32 sizes[] = {1, 31, 128, 257, 512};
    double maxError = 0, outputEnergy = 0, inputEnergy = 0;
    int64_t position = 0;
    size_t block = 0;
    while (position < static_cast<int64_t>(rate / 2)) {
        const UInt32 frames = sizes[block++ % 5];
        storage.buffers[0] = {1, frames * static_cast<UInt32>(sizeof(float)), left.data()};
        storage.buffers[1] = {1, frames * static_cast<UInt32>(sizeof(float)), right.data()};
        AudioTimeStamp timestamp{};
        timestamp.mFlags = kAudioTimeStampSampleTimeValid;
        timestamp.mSampleTime = static_cast<double>(position);
        AudioUnitRenderActionFlags flags = 0;
        check(AudioUnitRender(au, &flags, &timestamp, 0, frames,
                             reinterpret_cast<AudioBufferList *>(&storage)), "Render");
        for (UInt32 channel = 0; channel < 2; ++channel) {
            const auto *data = static_cast<float *>(storage.buffers[channel].mData);
            for (UInt32 i = 0; i < frames; ++i) {
                const float expected = sample(position + i - latency, channel, rate, listenSidechain);
                require(std::isfinite(data[i]), "Non-finite AU output");
                maxError = std::max(maxError, static_cast<double>(std::abs(data[i] - expected)));
                if (position > latency * 2) {
                    outputEnergy += data[i] * data[i];
                    inputEnergy += expected * expected;
                }
            }
        }
        position += frames;
    }
    if (suppress) require(outputEnergy < inputEnergy * 0.8, "AU Depth did not engage resonance suppression; energy ratio="
                           + std::to_string(outputEnergy / inputEnergy));
    else require(maxError < 0.0001, "AU reconstruction/routing error: " + std::to_string(maxError));
    check(AudioUnitUninitialize(au), "Uninitialize");

    // Saved state must survive both a parameter edit and a fresh AU instance.
    const auto saved = property<CFPropertyListRef>(au, kAudioUnitProperty_ClassInfo);
    const float savedDepth = get(au, params.at("Depth"));
    set(au, params.at("Depth"), 0.75f);
    require(get(au, params.at("Depth")) == 0.75f, "AU parameter change was lost");
    setProperty(au, kAudioUnitProperty_ClassInfo, saved);
    require(get(au, params.at("Depth")) == savedDepth, "AU state did not restore Depth");
    check(AudioComponentInstanceDispose(au), "Dispose");
    check(AudioComponentInstanceNew(component, &au), "New instance for state recall");
    setProperty(au, kAudioUnitProperty_ClassInfo, saved);
    require(get(au, params.at("Depth")) == savedDepth, "New instance did not restore Depth");
    require(get(au, params.at("Quality")) == quality, "New instance did not restore Quality");
    CFRelease(saved);
    // Verify the custom editor is advertised by the actual AU facade.
    const auto view = property<AudioUnitCocoaViewInfo>(au, kAudioUnitProperty_CocoaUI);
    require(view.mCocoaAUViewBundleLocation && view.mCocoaAUViewClass[0], "Missing Cocoa editor factory");
    CFRelease(view.mCocoaAUViewBundleLocation);
    CFRelease(view.mCocoaAUViewClass[0]);
    check(AudioComponentInstanceDispose(au), "Dispose restored instance");
}

static OSType fourCC(NSString *text) {
    require(text.length == 4, "Invalid component FourCC");
    const char *bytes = text.UTF8String;
    return (static_cast<UInt32>(bytes[0]) << 24) | (static_cast<UInt32>(bytes[1]) << 16)
        | (static_cast<UInt32>(bytes[2]) << 8) | static_cast<UInt32>(bytes[3]);
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        try {
            require(argc == 2, "Usage: lullaby_au_smoke /absolute/path/Lullaby.component");
            NSURL *url = [NSURL fileURLWithPath:[NSString stringWithUTF8String:argv[1]] isDirectory:YES];
            CFBundleRef bundle = CFBundleCreate(nullptr, (__bridge CFURLRef)url);
            require(bundle && CFBundleLoadExecutable(bundle), "Unable to load component bundle");
            NSDictionary *info = (__bridge NSDictionary *)CFBundleGetInfoDictionary(bundle);
            NSArray *components = info[@"AudioComponents"];
            require(components.count == 1, "Expected one AudioComponent registration");
            NSDictionary *entry = components[0];
            AudioComponentDescription description{fourCC(entry[@"type"]), fourCC(entry[@"subtype"]),
                                                  fourCC(entry[@"manufacturer"]), 0, 0};
            require(description.componentType == 'aufx' && description.componentSubType == 'Lulb'
                    && description.componentManufacturer == 'Llby', "Unexpected AU identity");
            auto factory = reinterpret_cast<AudioComponentFactoryFunction>(CFBundleGetFunctionPointerForName(
                bundle, (__bridge CFStringRef)entry[@"factoryFunction"]));
            require(factory, "Missing AU factory export");
            auto component = AudioComponentRegister(&description, (__bridge CFStringRef)entry[@"name"],
                                                     [entry[@"version"] unsignedIntValue], factory);
            require(component, "Unable to register AU in test process");
            for (double rate : {44100.0, 48000.0, 96000.0})
                for (int quality : {0, 3}) render(component, rate, quality, false);
            render(component, 48000, 0, true);
            render(component, 48000, 0, false, true);
            std::cout << "PASS: native AUv2 loading, 73 parameters, state recall, stereo rendering at "
                "44.1/48/96 kHz, variable blocks, two latency modes, sidechain routing, active suppression, Cocoa UI property.\n";
            // Registered factories must remain loaded until process exit.
            return 0;
        } catch (const std::exception &error) {
            std::cerr << "FAIL: " << error.what() << '\n';
            return 1;
        }
    }
}
