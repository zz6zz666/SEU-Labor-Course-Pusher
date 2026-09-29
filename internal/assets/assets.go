// Package assets embeds the application icons so the binary is self-contained.
package assets

import _ "embed"

//go:embed icon.png
var IconPNG []byte

//go:embed icon.ico
var IconICO []byte
