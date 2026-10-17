/* Narrow PipeWire ABI bridge. All protocol, bounds-checked pixel conversion and
 * file logic live in Rust. Built against local headers with GCC or Clang.
 * Callback pointers remain valid only until vmt_pw_close() returns. */
#include <pipewire/pipewire.h>
#include <spa/param/video/format-utils.h>
#include <spa/param/buffers.h>
#include <spa/buffer/meta.h>
#include <errno.h>
#include <stdlib.h>
#include <stdio.h>
#include <unistd.h>

struct vmt_raw_frame {
    const uint8_t *data;
    uint32_t maxsize, offset, size;
    int32_t stride;
    uint32_t width, height, format, crop_x, crop_y, crop_width, crop_height;
};
typedef void (*frame_cb)(void *, const struct vmt_raw_frame *);
typedef void (*error_cb)(void *, int, const char *);
struct vmt_capture {
    struct pw_thread_loop *loop;
    struct pw_context *context;
    struct pw_core *core;
    struct pw_stream *stream;
    struct spa_hook core_listener, stream_listener;
    struct spa_video_info_raw info;
    void *user;
    frame_cb frame;
    error_cb error;
    bool started, stopping, connected, formatted;
};
void vmt_pw_init(void) { pw_init(NULL, NULL); }
void vmt_pw_close(struct vmt_capture *c) {
    if (!c) return;
    /* Stop joins the loop thread; no callbacks can outlive Rust's user state. */
    if (c->started) pw_thread_loop_stop(c->loop);
    c->stopping = true;
    if (c->stream) pw_stream_destroy(c->stream);
    if (c->core) pw_core_disconnect(c->core);
    if (c->context) pw_context_destroy(c->context);
    if (c->loop) pw_thread_loop_destroy(c->loop);
    free(c);
}
static void core_error(void *data, uint32_t id, int seq, int res, const char *message) {
    struct vmt_capture *c = data;
    if (!c->stopping) c->error(c->user, 1, message ? message : "PipeWire core disconnected");
}
static void state_changed(void *data, enum pw_stream_state old, enum pw_stream_state state, const char *error) {
    struct vmt_capture *c = data;
    if (state == PW_STREAM_STATE_CONNECTING) c->connected = true;
    if (c->stopping) return;
    if (state == PW_STREAM_STATE_ERROR) c->error(c->user, 1, error ? error : "PipeWire stream error");
    if (state == PW_STREAM_STATE_UNCONNECTED && c->connected) c->error(c->user, 1, "PipeWire stream disconnected");
}
static uint32_t pixel_format(enum spa_video_format f) {
    switch (f) {
    case SPA_VIDEO_FORMAT_BGRA: return 0;
    case SPA_VIDEO_FORMAT_BGRx: return 1;
    case SPA_VIDEO_FORMAT_RGBA: return 2;
    case SPA_VIDEO_FORMAT_RGBx: return 3;
    case SPA_VIDEO_FORMAT_RGB: return 4;
    case SPA_VIDEO_FORMAT_BGR: return 5;
    default: return UINT32_MAX;
    }
}
static void param_changed(void *data, uint32_t id, const struct spa_pod *param) {
    struct vmt_capture *c = data;
    if (id != SPA_PARAM_Format) return;
    c->formatted = false;
    if (!param) return;
    if (spa_format_video_raw_parse(param, &c->info) < 0 || pixel_format(c->info.format) == UINT32_MAX ||
        (c->info.flags & SPA_VIDEO_FLAG_MODIFIER)) {
        c->error(c->user, 2, "unsupported PipeWire format or DMA-BUF modifier"); return;
    }
    c->formatted = true;
    uint8_t bytes[1024];
    struct spa_pod_builder b = SPA_POD_BUILDER_INIT(bytes, sizeof(bytes));
    const struct spa_pod *params[3];
    params[0] = spa_pod_builder_add_object(&b, SPA_TYPE_OBJECT_ParamBuffers, SPA_PARAM_Buffers,
        SPA_PARAM_BUFFERS_dataType, SPA_POD_CHOICE_FLAGS_Int((1u << SPA_DATA_MemPtr) | (1u << SPA_DATA_MemFd)));
    params[1] = spa_pod_builder_add_object(&b, SPA_TYPE_OBJECT_ParamMeta, SPA_PARAM_Meta,
        SPA_PARAM_META_type, SPA_POD_Id(SPA_META_Header), SPA_PARAM_META_size, SPA_POD_Int(sizeof(struct spa_meta_header)));
    params[2] = spa_pod_builder_add_object(&b, SPA_TYPE_OBJECT_ParamMeta, SPA_PARAM_Meta,
        SPA_PARAM_META_type, SPA_POD_Id(SPA_META_VideoCrop), SPA_PARAM_META_size, SPA_POD_Int(sizeof(struct spa_meta_region)));
    if (pw_stream_update_params(c->stream, params, 3) < 0) c->error(c->user, 2, "PipeWire buffer negotiation failed");
}
static void process(void *data) {
    struct vmt_capture *c = data;
    struct pw_buffer *p = NULL, *next;
    /* Discard stale queued buffers; never let decode/file I/O block this callback. */
    while ((next = pw_stream_dequeue_buffer(c->stream))) {
        if (p) pw_stream_queue_buffer(c->stream, p);
        p = next;
    }
    if (!p) return;
    struct spa_buffer *b = p->buffer;
    if (!c->formatted || b->n_datas != 1) goto done;
    const struct spa_meta_header *header = spa_buffer_find_meta_data(b, SPA_META_Header, sizeof(*header));
    if (header && (header->flags & (SPA_META_HEADER_FLAG_CORRUPTED | SPA_META_HEADER_FLAG_GAP))) goto done;
    const struct spa_data *d = &b->datas[0];
    if (!d->chunk || d->chunk->size == 0) goto done;
    if (d->chunk->flags & SPA_CHUNK_FLAG_CORRUPTED) goto done;
    if (!d->data) { c->error(c->user, 2, "PipeWire returned a non-mappable video buffer"); goto done; }
    struct vmt_raw_frame f = { .data = d->data, .maxsize = d->maxsize, .offset = d->chunk->offset,
        .size = d->chunk->size, .stride = d->chunk->stride, .width = c->info.size.width,
        .height = c->info.size.height, .format = pixel_format(c->info.format),
        .crop_width = c->info.size.width, .crop_height = c->info.size.height };
    const struct spa_meta_region *crop = spa_buffer_find_meta_data(b, SPA_META_VideoCrop, sizeof(*crop));
    if (crop && spa_meta_region_is_valid(crop)) {
        f.crop_x = crop->region.position.x; f.crop_y = crop->region.position.y;
        f.crop_width = crop->region.size.width; f.crop_height = crop->region.size.height;
    }
    c->frame(c->user, &f);
done:
    pw_stream_queue_buffer(c->stream, p);
}
static const struct pw_core_events core_events = { .version = PW_VERSION_CORE_EVENTS, .error = core_error };
static const struct pw_stream_events stream_events = { .version = PW_VERSION_STREAM_EVENTS,
    .state_changed = state_changed, .param_changed = param_changed, .process = process };

/* Takes ownership of fd on every path. Only a portal-authorized FD is accepted. */
struct vmt_capture *vmt_pw_open(int fd, uint32_t node, const char *serial,
                              void *user, frame_cb frame, error_cb error) {
    struct vmt_capture *c = calloc(1, sizeof(*c));
    if (!c) { close(fd); return NULL; }
    c->user = user; c->frame = frame; c->error = error;
    c->loop = pw_thread_loop_new("vmt-capture", NULL);
    if (!c->loop) goto fail;
    c->context = pw_context_new(pw_thread_loop_get_loop(c->loop), NULL, 0);
    if (!c->context) goto fail;
    c->core = pw_context_connect_fd(c->context, fd, NULL, 0);
    fd = -1; /* PipeWire owns the FD even on connect failure. */
    if (!c->core) goto fail;
    pw_core_add_listener(c->core, &c->core_listener, &core_events, c);
    struct pw_properties *props = pw_properties_new(PW_KEY_MEDIA_TYPE, "Video", PW_KEY_MEDIA_CATEGORY, "Capture",
        PW_KEY_MEDIA_ROLE, "Screen", PW_KEY_APP_NAME, "VMT visual receiver", NULL);
    if (!props) goto fail;
    if (serial) pw_properties_set(props, PW_KEY_TARGET_OBJECT, serial);
    c->stream = pw_stream_new(c->core, "VMT portal video", props);
    if (!c->stream) goto fail;
    pw_stream_add_listener(c->stream, &c->stream_listener, &stream_events, c);
    uint8_t bytes[1024];
    struct spa_pod_builder builder = SPA_POD_BUILDER_INIT(bytes, sizeof(bytes));
    const struct spa_pod *format = spa_pod_builder_add_object(&builder, SPA_TYPE_OBJECT_Format, SPA_PARAM_EnumFormat,
        SPA_FORMAT_mediaType, SPA_POD_Id(SPA_MEDIA_TYPE_video),
        SPA_FORMAT_mediaSubtype, SPA_POD_Id(SPA_MEDIA_SUBTYPE_raw),
        SPA_FORMAT_VIDEO_format, SPA_POD_CHOICE_ENUM_Id(6, SPA_VIDEO_FORMAT_BGRx, SPA_VIDEO_FORMAT_BGRA,
            SPA_VIDEO_FORMAT_RGBx, SPA_VIDEO_FORMAT_RGBA, SPA_VIDEO_FORMAT_RGB, SPA_VIDEO_FORMAT_BGR),
        SPA_FORMAT_VIDEO_size, SPA_POD_CHOICE_RANGE_Rectangle(&SPA_RECTANGLE(640,480), &SPA_RECTANGLE(1,1), &SPA_RECTANGLE(8192,8192)),
        SPA_FORMAT_VIDEO_framerate, SPA_POD_CHOICE_RANGE_Fraction(&SPA_FRACTION(0,1), &SPA_FRACTION(0,1), &SPA_FRACTION(60,1)));
    int rc = pw_stream_connect(c->stream, PW_DIRECTION_INPUT, serial ? PW_ID_ANY : node,
        PW_STREAM_FLAG_AUTOCONNECT | PW_STREAM_FLAG_MAP_BUFFERS, &format, 1);
    if (rc < 0) { errno = -rc; goto fail; }
    rc = pw_thread_loop_start(c->loop);
    if (rc < 0) { errno = -rc; goto fail; }
    c->started = true;
    return c;
fail:
    { int saved = errno ? errno : EIO; if (fd >= 0) close(fd); vmt_pw_close(c); errno = saved; }
    return NULL;
}
