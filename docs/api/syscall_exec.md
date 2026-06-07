# execve() Syscall

Execute a program.

## Synopsis

```c
int execve(const char *pathname, char *const argv[], char *const envp[]);
```

## Arguments

| Argument | Type | Description |
|----------|------|-------------|
| pathname | const char* | Path to the executable |
| argv | char*const[] | Array of argument strings |
| envp | char*const[] | Array of environment strings |

## Description

`execve()` replaces the current process's execution context with a new program loaded from `pathname`. The calling process's address space, registers, and file descriptor table are replaced. The process ID, parent process, and signal handler dispositions are preserved.

## ELF Loading Process

1. Kernel reads the ELF header from the file
2. Validates the ELF magic (`0x7fELF`) and architecture
3. Maps executable segments with appropriate permissions
4. Sets up the initial stack with argv and envp
5. Sets the instruction pointer to the entry point

## Return Value

On success, `execve()` does not return (the calling process is transformed). On error, -1 is returned and the calling process continues.

## Errors

| Error | Condition |
|-------|-----------|
| EACCES | File is not executable |
| ENOENT | File not found |
| ENOEXEC | Invalid ELF format or architecture mismatch |
| ENOMEM | Insufficient memory for loading |
| ETXTBSY | Executable is open for writing |
