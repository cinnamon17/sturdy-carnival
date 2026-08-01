#!/bin/bash

find src/ -name "*.rs" -exec head -n 9999 {} + > contexto.txt
