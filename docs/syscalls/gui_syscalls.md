# GUI System Calls

SkyOS provides kernel-level GUI operations through syscalls 300-306.

## skyos_create_window (syscall 300)

```c
int skyos_create_window(int x, int y, int width, int height, uint32_t flags);
```

Creates a new window at the specified position and size. The window is initially hidden; the compositor makes it visible after the first buffer flush. Returns a window ID (positive integer) on success.

**Flags**:
- `WINDOW_RESIZABLE` (1): Window can be resized by the user
- `WINDOW_BORDERLESS` (2): No window decorations
- `WINDOW_TRANSPARENT` (4): Support alpha transparency
- `WINDOW_FULLSCREEN` (8): Full-screen window

## skyos_get_buffer (syscall 301)

```c
void *skyos_get_buffer(int window_id);
```

Returns a pointer to the window's framebuffer in the calling process's address space. The buffer is in BGRA32 format with 8 bits per channel (32 bits per pixel). The buffer size is `width * height * 4` bytes. Returns NULL if the window ID is invalid.

## skyos_flush (syscall 302)

```c
int skyos_flush(int window_id, int x, int y, int width, int height);
```

Marks a rectangular region of the window as dirty and requests the compositor to update the display. The compositor may clip the rectangle to the window bounds.

## skyos_map_buffer (syscall 303)

```c
void *skyos_map_buffer(size_t size, uint32_t flags);
```

Maps a GPU-accessible memory buffer. Used for hardware-accelerated rendering. The returned pointer is to write-combined memory that is coherent with the GPU.

**Flags**:
- `MAP_GPU_WRITE_COMBINE` (0): Write-combining for GPU writes
- `MAP_GPU_UNCACHED` (1): Uncached for MMIO access
- `MAP_GPU_LARGE_PAGES` (2): Use 2 MiB pages if available

## skyos_get_display_info (syscall 304)

```c
int skyos_get_display_info(struct display_info *info);
```

Returns display properties: resolution, refresh rate, color depth, and physical size. Returns 0 on success.

## skyos_set_cursor (syscall 305)

```c
int skyos_set_cursor(int window_id, int x, int y);
```

Sets the cursor position relative to the specified window. Used by the compositor to manage cursor state.

## skyos_event_wait (syscall 306)

```c
int skyos_event_wait(struct input_event *events, int max_events, uint64_t timeout_ns);
```

Blocks until input events are available or the timeout expires. Returns the number of events read. Timeout of 0 returns immediately (non-blocking poll).
