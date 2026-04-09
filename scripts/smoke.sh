#!/bin/sh
set -eu

exec cargo run -- smoke-aws-ecs
