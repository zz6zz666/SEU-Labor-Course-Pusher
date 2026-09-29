package main

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"

	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/appwindow"
	"github.com/zz6zz666/SEU-Labor-Course-Pusher/internal/osutil"
)

const (
	wizardUITitle  = "设置向导 · SEU 劳动教育课程推送助手"
	wizardUIIconID = 32512
	wizardUIWidth  = 980
	wizardUIHeight = 720
	wizardUIMinW   = 860
	wizardUIMinH   = 640
)

// runWizardUI hosts the settings window in this short-lived process using the
// system WebView2 runtime. Keeping it out of the resident daemon means the
// daemon never loads WebView2 (and its ~55 MB in-process footprint), and every
// byte is reclaimed when the window closes.
func runWizardUI(url, dataPath string) error {
	if url == "" {
		return fmt.Errorf("缺少向导地址")
	}
	if dataPath == "" {
		dataPath = filepath.Join(os.TempDir(), "seu-labor-wizard-webview")
	}
	scale := osutil.DPIScale()
	if scale < 1 {
		scale = 1
	}
	px := func(v int) int { return int(float64(v) * scale) }

	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	w, err := appwindow.Open(url, appwindow.Options{
		Title:     wizardUITitle,
		Width:     px(wizardUIWidth),
		Height:    px(wizardUIHeight),
		MinWidth:  px(wizardUIMinW),
		MinHeight: px(wizardUIMinH),
		IconID:    wizardUIIconID,
		DataPath:  dataPath,
	})
	if err != nil {
		return err
	}
	w.Run()
	return nil
}
