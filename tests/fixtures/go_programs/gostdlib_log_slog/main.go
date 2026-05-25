package main

import (
	"fmt"
	"log/slog"
)

func main() {
	fmt.Println(slog.LevelDebug < slog.LevelInfo, slog.LevelWarn > slog.LevelError)
}
