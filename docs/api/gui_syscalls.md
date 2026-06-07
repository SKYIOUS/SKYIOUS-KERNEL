# GUI Subsystem Syscalls

SkyOS provides kernel-level syscalls for the GUI compositor subsystem.

## skyos_create_window (syscall 300)

Create a new window.

```c
int skyos_create_window(int x, int y, int width, int height, uint32_t flags);
```

Returns a window ID on success, or -1 on error.

## skyos_get_buffer (syscall 301)

Get the framebuffer for a window.

```c
void* skyos_get_buffer(int window_id);
```

Returns a pointer to the window's pixel buffer (BGRA format, 32 bits per pixel), or NULL if the window ID is invalid.

## skyos_flush (syscall 302)

Flush window updates to the display.

```c
int skyos_flush(int window_id, int x, int y, int width, int height);
```

The dirty rectangle specifies which portion of the window needs updating. Returns 0 on success.

## skyos_map_buffer (syscall 303)

Map a GPU-accessible buffer.

```c
void* skyos_map_buffer(size_t size, uint32_t flags);
```

Returns a pointer to a shared buffer that can be used for DMA with the GPU. The flags parameter controls caching behavior (write-combine, uncached, etc.).

## Event Handling

The display server receives input events through a dedicated event queue accessible via `read()` on a special device file. Events include:

```c
struct input_event {
    uint32_t type;      // EV_KEY, EV_MOTION, EV_SCROLL
    uint32_t code;      // Key code, button number, or axis
    int32_t value;      // Press/release state or delta
};
```
