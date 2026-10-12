// A minimal Cocoa parent for manually checking the real CLAP editor.
// clang -fobjc-arc -framework Cocoa -framework AVFoundation scripts/macos_gui_smoke.m -o /tmp/lullaby-gui-smoke
// /tmp/lullaby-gui-smoke "$PWD/target/release/liblullaby.dylib"
// Close the window to finish. The plugin is never installed into a system folder.
#import <Cocoa/Cocoa.h>
#import <AVFoundation/AVFoundation.h>
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <assert.h>
#include <math.h>

typedef struct { uint32_t major, minor, revision; } Version;
typedef struct Host Host;
typedef struct Plugin Plugin;
struct Host {
    Version version; void *data;
    const char *name, *vendor, *url, *host_version;
    const void *(*extension)(const Host *, const char *);
    void (*restart)(const Host *), (*process)(const Host *), (*callback)(const Host *);
};
struct Plugin {
    const void *descriptor; void *data;
    bool (*init)(const Plugin *);
    void (*destroy)(const Plugin *);
    bool (*activate)(const Plugin *, double, uint32_t, uint32_t);
    void (*deactivate)(const Plugin *);
    bool (*start)(const Plugin *);
    void (*stop)(const Plugin *), (*reset)(const Plugin *);
    int32_t (*process)(const Plugin *, const void *);
    const void *(*extension)(const Plugin *, const char *);
    void (*main_thread)(const Plugin *);
};
typedef struct {
    Version version; bool (*init)(const char *); void (*deinit)(void);
    const void *(*factory)(const char *);
} Entry;
typedef struct {
    uint32_t (*count)(const void *);
    const void *(*descriptor)(const void *, uint32_t);
    const Plugin *(*create)(const void *, const Host *, const char *);
} Factory;
typedef struct { const char *api; void *handle; } Window;
typedef struct {
    bool (*supported)(const Plugin *, const char *, bool);
    bool (*preferred)(const Plugin *, const char **, bool *);
    bool (*create)(const Plugin *, const char *, bool);
    void (*destroy)(const Plugin *);
    bool (*scale)(const Plugin *, double);
    bool (*size)(const Plugin *, uint32_t *, uint32_t *);
    bool (*can_resize)(const Plugin *);
    bool (*hints)(const Plugin *, void *);
    bool (*adjust)(const Plugin *, uint32_t *, uint32_t *);
    bool (*set_size)(const Plugin *, uint32_t, uint32_t);
    bool (*parent)(const Plugin *, const Window *);
    bool (*transient)(const Plugin *, const Window *);
    void (*title)(const Plugin *, const char *);
    bool (*show)(const Plugin *), (*hide)(const Plugin *);
} Gui;

static const Plugin *plugin;
static const Gui *gui;
static NSWindow *window;
static bool processing;
static void no_op(const Host *h) {}
static bool resize(const Host *h, uint32_t width, uint32_t height) {
    if (window) [window setContentSize:NSMakeSize(width, height)];
    return true;
}
static bool show(const Host *h) { [window makeKeyAndOrderFront:nil]; return true; }
static bool hide(const Host *h) { [window orderOut:nil]; return true; }
static void closed(const Host *h, bool destroyed) {}
static const struct {
    void (*hints)(const Host *);
    bool (*resize)(const Host *, uint32_t, uint32_t);
    bool (*show)(const Host *), (*hide)(const Host *);
    void (*closed)(const Host *, bool);
} host_gui = {no_op, resize, show, hide, closed};
static uint32_t event_count(const void *events) { return 0; }
static const void *get_event(const void *events, uint32_t i) { return NULL; }
static bool push_event(const void *events, const void *event) { return true; }
static void rescan(const Host *h, uint32_t flags) {}
static void clear_param(const Host *h, uint32_t id, uint32_t flags) {}
static void flush(const Host *h) {
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!plugin) return;
        if (processing) return; // process() consumes pending GUI parameter events.
        const struct { void *count, *info, *get, *to_text, *from_text;
            void (*flush)(const Plugin *, const void *, const void *); } *params = plugin->extension(plugin, "clap.params");
        const struct { void *ctx; uint32_t (*size)(const void *); const void *(*get)(const void *, uint32_t); } in = {NULL, event_count, get_event};
        const struct { void *ctx; bool (*push)(const void *, const void *); } out = {NULL, push_event};
        params->flush(plugin, &in, &out);
    });
}
static const struct {
    void (*rescan)(const Host *, uint32_t);
    void (*clear)(const Host *, uint32_t, uint32_t);
    void (*flush)(const Host *);
} host_params = {rescan, clear_param, flush};
static const void *extension(const Host *h, const char *id) {
    if (strcmp(id, "clap.gui") == 0) return &host_gui;
    if (strcmp(id, "clap.params") == 0) return &host_params;
    return NULL;
}
static void callback(const Host *h) {
    dispatch_async(dispatch_get_main_queue(), ^{ if (plugin) plugin->main_thread(plugin); });
}

@interface Delegate : NSObject <NSApplicationDelegate, NSWindowDelegate>
@end

// Optional, silent analysis of a local fixture exercises the actual DSP/meters.
// It never opens an audio device or sends sound to the speakers.
typedef struct {
    float **data32; double **data64; uint32_t channels, latency; uint64_t constant_mask;
} ClapAudioBuffer;
typedef struct {
    int64_t steady_time; uint32_t frames; const void *transport;
    const ClapAudioBuffer *inputs; ClapAudioBuffer *outputs; uint32_t input_count, output_count;
    const void *in_events; const void *out_events;
} Process;

static NSTimer *start_analysis(NSString *path) {
    if (![[NSFileManager defaultManager] fileExistsAtPath:path]) return nil;
    NSError *error = nil;
    AVAudioFile *file = [[AVAudioFile alloc] initForReading:[NSURL fileURLWithPath:path]
        commonFormat:AVAudioPCMFormatFloat32 interleaved:NO error:&error];
    if (!file || file.processingFormat.channelCount != 2 || file.length < 512) return nil;
    AVAudioPCMBuffer *buffer = [[AVAudioPCMBuffer alloc] initWithPCMFormat:file.processingFormat
        frameCapacity:(AVAudioFrameCount)file.length];
    if (![file readIntoBuffer:buffer error:&error]) return nil;
    double rate = file.processingFormat.sampleRate;
    if (!plugin->activate(plugin, rate, 1, 512) || !plugin->start(plugin)) return nil;
    processing = true;
    __block uint32_t position = 0;
    __block int64_t steady = 0;
    return [NSTimer scheduledTimerWithTimeInterval:512.0 / rate repeats:YES block:^(NSTimer *timer) {
        float left[512] = {0}, right[512] = {0}, silence[512] = {0};
        float *main[2] = {buffer.floatChannelData[0] + position, buffer.floatChannelData[1] + position};
        float *sidechain[2] = {silence, silence};
        float *output[2] = {left, right};
        ClapAudioBuffer inputs[2] = {{main, NULL, 2, 0, 0}, {sidechain, NULL, 2, 0, UINT64_MAX}};
        ClapAudioBuffer outputs[1] = {{output, NULL, 2, 0, 0}};
        const struct { void *ctx; uint32_t (*size)(const void *); const void *(*get)(const void *, uint32_t); } in = {NULL, event_count, get_event};
        const struct { void *ctx; bool (*push)(const void *, const void *); } out = {NULL, push_event};
        Process process = {steady, 512, NULL, inputs, outputs, 2, 1, &in, &out};
        float peak = 0;
        for (int i = 0; i < 512; ++i) peak = fmaxf(peak, fabsf(main[0][i]));
        assert(plugin->process(plugin, &process) != 0);
        if (steady % 25600 == 0) window.title = [NSString stringWithFormat:
            @"Lullaby — native CLAP editor · vocal %.1f s · input %.0f dBFS", position / rate,
            20 * log10(fmaxf(peak, 1e-9))];
        steady += 512;
        position += 512;
        if (position + 512 > buffer.frameLength) position = 0;
    }];
}
@implementation Delegate
- (BOOL)applicationShouldTerminateAfterLastWindowClosed:(NSApplication *)sender { return YES; }
- (void)windowDidResize:(NSNotification *)notification {
    if (gui && plugin) {
        NSSize size = window.contentView.bounds.size;
        gui->set_size(plugin, (uint32_t)size.width, (uint32_t)size.height);
    }
}
@end

int main(int argc, char **argv) {
    @autoreleasepool {
        NSString *libraryPath = argc >= 2 ? [NSString stringWithUTF8String:argv[1]] :
            [[[NSBundle mainBundle] resourcePath] stringByAppendingPathComponent:@"liblullaby.dylib"];
        [NSApplication sharedApplication];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyRegular];
        Delegate *delegate = [Delegate new];
        NSApp.delegate = delegate;
        void *library = dlopen(libraryPath.fileSystemRepresentation, RTLD_NOW | RTLD_LOCAL);
        if (!library) { fprintf(stderr, "%s\n", dlerror()); return 1; }
        const Entry *entry = dlsym(library, "clap_entry");
        assert(entry && entry->init(libraryPath.fileSystemRepresentation));
        const Factory *factory = entry->factory("clap.plugin-factory");
        Host host = {{1, 2, 5}, NULL, "Lullaby GUI smoke", "Lullaby", "", "1.0", extension, no_op, no_op, callback};
        plugin = factory->create(factory, &host, "audio.lullaby.resonance");
        assert(plugin && plugin->init(plugin));
        gui = plugin->extension(plugin, "clap.gui");
        assert(gui && gui->supported(plugin, "cocoa", false));
        assert(gui->create(plugin, "cocoa", false));
        uint32_t width = 0, height = 0;
        assert(gui->size(plugin, &width, &height));
        window = [[NSWindow alloc] initWithContentRect:NSMakeRect(0, 0, width, height)
            styleMask:NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskResizable
            backing:NSBackingStoreBuffered defer:NO];
        window.title = @"Lullaby — native CLAP editor smoke test";
        window.delegate = delegate;
        window.contentMinSize = NSMakeSize(960, 640);
        Window parent = {"cocoa", (__bridge void *)window.contentView};
        assert(gui->parent(plugin, &parent));
        assert(gui->show(plugin));
        NSString *audioPath = argc >= 3 ? [NSString stringWithUTF8String:argv[2]] :
            [[[NSBundle mainBundle] resourcePath] stringByAppendingPathComponent:@"preview.wav"];
        NSTimer *analysis = start_analysis(audioPath);
        if (analysis) [[NSRunLoop mainRunLoop] addTimer:analysis forMode:NSRunLoopCommonModes];
        window.title = analysis ? @"Lullaby — native CLAP editor · vocal analysis" :
            @"Lullaby — native CLAP editor · no audio fixture";
        [window center];
        [window makeKeyAndOrderFront:nil];
        [NSApp activateIgnoringOtherApps:YES];
        [NSApp run];
        [analysis invalidate];
        if (processing) { plugin->stop(plugin); plugin->deactivate(plugin); processing = false; }
        gui->destroy(plugin);
        gui = NULL;
        plugin->destroy(plugin);
        plugin = NULL;
        entry->deinit();
        // Do not dlclose GUI libraries: queued OS callbacks may still reference them.
    }
    return 0;
}
