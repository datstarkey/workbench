/*
 * macOS exec shim for terminal shells (see src/pty.rs).
 *
 * posix_spawn already made this process a session leader (POSIX_SPAWN_SETSID)
 * with the PTY slave on stdio, but XNU runs spawn file actions in the parent's
 * context, so opening the slave there never makes it the controlling terminal.
 * Claim it here, then become the shell: execv keeps the pid, session, signal
 * dispositions, cwd and environment.
 *
 * argv: workbench-pty-exec <program path> <argv0> [args...]
 */
#include <stdio.h>
#include <sys/ioctl.h>
#include <unistd.h>

int main(int argc, char **argv) {
    if (argc < 3) {
        fputs("workbench-pty-exec: missing program\n", stderr);
        return 127;
    }
    if (ioctl(STDIN_FILENO, TIOCSCTTY, 0) == -1) {
        perror("workbench-pty-exec: TIOCSCTTY");
        return 126;
    }
    execv(argv[1], argv + 2);
    perror("workbench-pty-exec: exec");
    return 127;
}
