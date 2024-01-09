#!/bin/bash

curl -L   -H "Accept: application/vnd.github+json"   -H "Authorization: Bearer $(cat TOKEN.txt)"   -H "X-GitHub-Api-Version: 2022-11-28"     https://api.github.com/repos/fuulish/pong/pulls/2/comments
