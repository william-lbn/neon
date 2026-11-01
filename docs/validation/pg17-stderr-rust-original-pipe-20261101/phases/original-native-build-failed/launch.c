#include <sys/prctl.h>
#include <unistd.h>
#include <stdio.h>
int main(int argc,char **argv){if(argc<2||prctl(PR_SET_NO_NEW_PRIVS,1,0,0,0)){perror("NNP");return 91;}execv(argv[1],argv+1);perror("exec PG");return 92;}
