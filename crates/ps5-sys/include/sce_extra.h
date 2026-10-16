/* Sony system-library functions that the SDK resolves at link time but
 * ships no header for. Signatures are copied from the SDK samples. Each
 * group notes the .sprx it lives in; the final link needs -l<name>
 * (libkernel*, libSceLibcInternal and libSceNet are linked by default). */
#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <sys/types.h>

/* ---- libkernel ----------------------------------------------------- */

typedef struct notify_request {
  char useless1[45];
  char message[3075];
} notify_request_t;

typedef struct app_info {
  uint32_t app_id;
  uint64_t unknown1;
  uint32_t app_type;
  char     title_id[10];
  char     unknown2[0x3c];
} app_info_t;

int  sceKernelSendNotificationRequest(int, notify_request_t*, size_t, int);
int  sceKernelGetAppInfo(pid_t pid, app_info_t *info);
/* these five need -lkernel_sys */
int  sceKernelGetHwModelName(char *);
int  sceKernelGetHwSerialNumber(char *);
long sceKernelGetCpuFrequency(void);
int  sceKernelGetCpuTemperature(int *);
int  sceKernelGetSocSensorTemperature(int, int *);

/* ---- libSceNotification (-lSceNotification) ----------------------- */

#define SCE_NOTIFICATION_LOCAL_USER_ID_SYSTEM 0xFE
int sceNotificationSend(int userId, bool isLogged, const char *payload);

/* ---- libSceRandom (-lSceRandom) ------------------------------------ */

int sceRandomGetRandomNumber(uint8_t *buf, size_t size);

/* ---- libSceUserService (-lSceUserService) -------------------------- */

int sceUserServiceInitialize(void *);
int sceUserServiceTerminate(void);

/* ---- libSceSystemService (-lSceSystemService) ---------------------- */

int sceSystemServiceLaunchWebBrowser(const char *uri, void *);

/* ---- libSceAppInstUtil (-lSceIpmi -lSceAppInstUtil + user/system svc) */

int sceAppInstUtilInitialize(void);
int sceAppInstUtilAppInstallTitleDir(const char *, const char *, void *);

/* ---- libSceNet (default) ------------------------------------------- */

int sceNetInit(void);
int sceNetPoolCreate(const char *, int, int);
int sceNetPoolDestroy(int);

/* ---- libSceSsl (-lSceSsl) ------------------------------------------ */

int sceSslInit(size_t);
int sceSslTerm(int);

/* ---- libSceHttp2 (-lSceHttp2) -------------------------------------- */

int sceHttp2Init(int, int, size_t, int);
int sceHttp2Term(int);
int sceHttp2CreateTemplate(int, const char *, int, int);
int sceHttp2DeleteTemplate(int);
int sceHttp2CreateRequestWithURL(int, const char *, const char *, uint64_t);
int sceHttp2DeleteRequest(int);
int sceHttp2SendRequest(int, const void *, size_t);
int sceHttp2GetStatusCode(int, int *);
int sceHttp2ReadData(int, void *, size_t);
