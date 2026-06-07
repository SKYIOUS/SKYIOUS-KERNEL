# GUI Compositor Design Choices

The SkyOS GUI subsystem uses a client-server compositing architecture.

## Architecture Overview

The display server runs as a privileged userspace process. Applications (clients) connect to the server and create windows through kernel syscalls. The compositor manages window surfaces and renders the final display.

## Window Buffers

Each window has a backing buffer allocated by the kernel's memory manager. The buffer is accessible by both the application (for drawing) and the compositor (for compositing). Window buffers use BGRA32 pixel format with pre-multiplied alpha.

## Compositing

The compositor uses a damage-based rendering approach:
1. Applications signal dirty regions via `skyos_flush()`
2. The compositor collects dirty rectangles from all windows
3. Only damaged regions are re-composited
4. The final frame is presented to the display hardware

## Design Decisions

1. **No client-side decorations**: The compositor handles window decoration, consistent across applications.

2. **Direct buffer access**: Applications draw directly into their window buffer, avoiding IPC overhead for pixel data.

3. **Hardware acceleration**: When available, the compositor uses GPU blit operations for compositing. A software fallback using the framebuffer is always available.

4. **Input handling**: The compositor receives raw input events (from the PS/2 or USB drivers) and routes them to the appropriate window based on focus and cursor position.

## Display Protocol

Communication between the compositor and applications uses a shared-memory protocol:
- Window creation/destruction: kernel syscalls
- Buffer updates: shared memory with dirty rectangle events
- Input events: shared event queue
- Window management: socket-based IPC for resize, move, minimize
