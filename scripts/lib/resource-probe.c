// 仅供外部验收；使用当前 macOS SDK 的结构体，不把探针编进正式应用。
#include <libproc.h>
#include <sys/resource.h>
#include <sys/proc_info.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <mach/mach_time.h>

// XNU 的 PROC_PIDCOALITIONINFO (20)，sys/proc_info.h 的 PRIVATE ABI。
// 只在外部测量器使用；尺寸不符/被拒绝就输出 null，不让正式应用依赖它。
// https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/proc_info.h
struct lite_coalition_info { uint64_t ids[2]; uint64_t reserved[3]; };
_Static_assert(sizeof(struct lite_coalition_info) == 40, "coalition ABI");

static void string(const char *s) {
    putchar('"');
    for (const unsigned char *p = (const unsigned char *)s; *p; ++p) {
        if (*p == '"' || *p == '\\') putchar('\\');
        if (*p < 32) printf("\\u%04x", *p); else putchar(*p);
    }
    putchar('"');
}

static void sample(int pid) {
    struct rusage_info_v4 r = {0};
    printf("{\"pid\":%d", pid);
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&r) != 0) {
        printf(",\"error\":%d}", errno); return;
    }
    mach_timebase_info_data_t scale;
    if (mach_timebase_info(&scale) != 0 || !scale.denom) {
        printf(",\"error\":%d}", EIO); return;
    }
    // rusage 的 CPU 时间是 Mach ticks；Apple Silicon 上不能直接当纳秒。
    unsigned long long cpu_ns = (unsigned long long)(((__uint128_t)r.ri_user_time + r.ri_system_time) * scale.numer / scale.denom);
    printf(",\"start\":%llu,\"cpu_ns\":%llu,\"footprint\":%llu,\"rss\":%llu,"
           "\"disk_read\":%llu,\"disk_write\":%llu,\"logical_write\":%llu,\"idle_wakeups\":%llu",
           r.ri_proc_start_abstime, cpu_ns,
           r.ri_phys_footprint, r.ri_resident_size, r.ri_diskio_bytesread,
           r.ri_diskio_byteswritten, r.ri_logical_writes, r.ri_pkg_idle_wkups);
    struct proc_taskinfo t = {0};
    if (proc_pidinfo(pid, PROC_PIDTASKINFO, 0, &t, sizeof(t)) == sizeof(t))
        printf(",\"threads\":%d", t.pti_threadnum);
    else printf(",\"threads\":null");
    int n = proc_pidinfo(pid, PROC_PIDLISTFDS, 0, NULL, 0);
    if (n > 0 && n < 1024 * 1024) {
        int cap = n + 32 * (int)sizeof(struct proc_fdinfo);
        void *fds = malloc((size_t)cap);
        int got = fds ? proc_pidinfo(pid, PROC_PIDLISTFDS, 0, fds, cap) : -1;
        if (got > 0 && got < cap) printf(",\"fds\":%zu", (size_t)got / sizeof(struct proc_fdinfo));
        else printf(",\"fds\":null");
        free(fds);
    } else printf(",\"fds\":null");
    putchar('}');
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "--catalog <test-executable> | --sample <pid>...\n"); return 2; }
    putchar('[');
    if (strcmp(argv[1], "--catalog") == 0) {
        int n = proc_listallpids(NULL, 0);
        if (n <= 0 || n > 100000) return 1;
        int cap = n + 256;
        pid_t *pids = calloc((size_t)cap, sizeof(pid_t));
        if (!pids) return 1;
        n = proc_listallpids(pids, cap * (int)sizeof(pid_t));
        if (n < 0 || n >= cap) { free(pids); return 1; }
        int first = 1;
        for (int i = 0; i < n; ++i) {
            struct proc_bsdinfo b = {0}; char path[PROC_PIDPATHINFO_MAXSIZE] = {0};
            if (proc_pidinfo(pids[i], PROC_PIDTBSDINFO, 0, &b, sizeof(b)) != sizeof(b)) continue;
            if (proc_pidpath(pids[i], path, sizeof(path)) <= 0) continue;
            if (!first) putchar(','); first = 0;
            const char *name = strrchr(path, '/'); name = name ? name + 1 : path;
            printf("{\"pid\":%d,\"ppid\":%u,\"name\":", pids[i], b.pbi_ppid); string(name);
            printf(",\"app\":%s,\"webkit\":%s", strcmp(path, argv[2]) == 0 ? "true" : "false",
                   strstr(name, "com.apple.WebKit.") ? "true" : "false");
            struct lite_coalition_info coal = {0};
            if (proc_pidinfo(pids[i], 20, 0, &coal, sizeof(coal)) == sizeof(coal) && coal.ids[0] && coal.ids[1])
                printf(",\"resource_coalition\":%llu,\"jetsam_coalition\":%llu}", (unsigned long long)coal.ids[0], (unsigned long long)coal.ids[1]);
            else printf(",\"resource_coalition\":null,\"jetsam_coalition\":null}");
        }
        free(pids);
    } else if (strcmp(argv[1], "--sample") == 0) {
        for (int i = 2; i < argc; ++i) {
            char *end = NULL; long pid = strtol(argv[i], &end, 10);
            if (!end || *end || pid <= 0 || pid > 2147483647) return 2;
            if (i > 2) putchar(','); sample((int)pid);
        }
    } else return 2;
    puts("]"); return 0;
}
