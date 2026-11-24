/* Log the start and the exit of one program, then become it (macOS).
 *
 * trace.py puts a copy of this program in the place of each shared test
 * launcher and of `git` and `tmux` on the PATH. The program keeps its pid:
 * it forks a watcher, then execs `<its own path>.orig` with the same argv[0].
 * The watcher leaves the process group, closes every inherited descriptor,
 * waits for the exit with kqueue and appends one line to $TRACE_LOG:
 *
 *     <pid> <start µs> <exit µs> <basename of argv[0]>
 *
 * A `.orig` that starts with `#!` is a shell launcher that reads `$0`, so it
 * runs as `sh -c '. <orig>' <argv[0]> <args>`, which keeps `$0`.
 */
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/event.h>
#include <sys/time.h>
#include <unistd.h>

static long long now_us(void) {
    struct timeval tv;
    gettimeofday(&tv, NULL);
    return (long long)tv.tv_sec * 1000000 + tv.tv_usec;
}

int main(int argc, char **argv) {
    (void)argc;
    long long start = now_us();
    char self[PATH_MAX], orig[PATH_MAX + 8];
    if (!realpath(argv[0], self)) {
        /* argv[0] without a slash came through the PATH. */
        char *path = getenv("PATH"), *copy = strdup(path ? path : ""), *dir;
        int found = 0;
        for (dir = strtok(copy, ":"); dir && !found; dir = strtok(NULL, ":")) {
            char candidate[PATH_MAX];
            snprintf(candidate, sizeof candidate, "%s/%s", dir, argv[0]);
            found = access(candidate, X_OK) == 0 && realpath(candidate, self);
        }
        if (!found) return 127;
    }
    snprintf(orig, sizeof orig, "%s.orig", self);
    const char *name = strrchr(argv[0], '/') ? strrchr(argv[0], '/') + 1 : argv[0];
    const char *log = getenv("TRACE_LOG");
    pid_t target = getpid();
    int ready[2];
    if (log && pipe(ready) == 0) {
        pid_t watcher = fork();
        if (watcher == 0) {
            setsid();
            int kq = kqueue();
            struct kevent change;
            EV_SET(&change, target, EVFILT_PROC, EV_ADD | EV_ONESHOT, NOTE_EXIT, 0, NULL);
            kevent(kq, &change, 1, NULL, 0, NULL);
            write(ready[1], "", 1);
            for (int fd = 0; fd < 1024; fd++)
                if (fd != kq) close(fd);
            struct kevent event;
            kevent(kq, NULL, 0, &event, 1, NULL);
            long long end = now_us();
            int out = open(log, O_WRONLY | O_APPEND | O_CREAT, 0644);
            char line[512];
            int n = snprintf(line, sizeof line, "%d %lld %lld %s\n", target, start, end, name);
            write(out, line, n);
            _exit(0);
        }
        char byte;
        close(ready[1]);
        read(ready[0], &byte, 1);
        close(ready[0]);
    }
    char head[2] = {0};
    int fd = open(orig, O_RDONLY);
    if (fd >= 0) {
        read(fd, head, 2);
        close(fd);
    }
    if (head[0] == '#' && head[1] == '!') {
        char script[PATH_MAX + 32];
        snprintf(script, sizeof script, ". '%s'", orig);
        char **shell = calloc(argc + 4, sizeof *shell);
        shell[0] = "sh";
        shell[1] = "-c";
        shell[2] = script;
        for (int i = 0; i < argc; i++) shell[3 + i] = argv[i];
        execv("/bin/sh", shell);
    } else {
        execv(orig, argv);
    }
    return 127;
}
