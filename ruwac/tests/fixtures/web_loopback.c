/* Test-only process-local loopback translation for two generated SSH identities.
 * No production process loads this fixture. Original SSH forwards request the
 * same explicit loopback port; each isolated server redirects only that exact tuple.
 */
#define _GNU_SOURCE
#include <arpa/inet.h>
#include <dlfcn.h>
#include <errno.h>
#include <stdlib.h>
#include <sys/socket.h>
int connect(int fd,const struct sockaddr *address,socklen_t length) {
 int (*original)(int,const struct sockaddr*,socklen_t)=dlsym(RTLD_NEXT,"connect");
 if (!original) { errno=ENOSYS; return -1; }
 if (address && length>=sizeof(struct sockaddr_in) && address->sa_family==AF_INET) {
  const struct sockaddr_in *v=(const struct sockaddr_in*)address;
  const char *setting=getenv("RUWAC_FIXTURE_HTTP_PORT");
  const char *requested=getenv("RUWAC_FIXTURE_REQUEST_PORT");
  unsigned long wanted=requested?strtoul(requested,0,10):0;
  if (setting && v->sin_addr.s_addr==htonl(INADDR_LOOPBACK) && wanted>0 && wanted<=65535 && v->sin_port==htons((unsigned short)wanted)) {
   char *end; unsigned long port=strtoul(setting,&end,10);
   if (end!=setting && !*end && port>0 && port<=65535) {
    struct sockaddr_in mapped=*v; mapped.sin_port=htons((unsigned short)port);
    return original(fd,(const struct sockaddr*)&mapped,sizeof(mapped));
   }
  }
 }
 return original(fd,address,length);
}
