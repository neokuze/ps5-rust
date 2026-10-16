/* Everything ps5-sys exposes. build.rs allowlists by file, so adding a
 * header here is enough to bind it (if its path matches the allowlist). */

/* POSIX first: some SDK headers (iommu.h) use size_t without including it.
 * POSIX / FreeBSD libc commonly used by payloads */
#include <dirent.h>
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <netinet/in.h>
#include <sys/mman.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/sysctl.h>

/* SDK: all PS5-specific headers */
#include <ps5/iommu.h>
#include <ps5/kernel.h>
#include <ps5/klog.h>
#include <ps5/mdbg.h>
#include <ps5/nid.h>
#include <ps5/payload.h>

/* Sony libs without SDK headers */
#include "sce_extra.h"
