#!/bin/bash

curl -X POST -H "Accept: application/vnd.github+json"   -H "Authorization: Bearer $(cat TOKEN.txt)"   -H "X-GitHub-Api-Version: 2022-11-28"     https://api.github.ibm.com/repos/Frank-Uhlig1/prtest/pulls/1/comments/8177350/replies -d '{"body":"yes, I am"}'
