
#ifdef __cplusplus
extern "C" {
#endif

typedef struct DIR DIR;
struct dirent { char d_name[256]; };
DIR *opendir(const char *);
struct dirent *readdir(DIR *);
int closedir(DIR *);

#ifdef __cplusplus
}
#endif
