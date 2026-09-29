//go:build !windows

package ui

import "errors"

type Tray struct{ actions Actions }

func NewTray(actions Actions, _ string) (*Tray, error) {
	return &Tray{actions: actions}, nil
}

func (t *Tray) Update()    {}
func (t *Tray) Stop()      {}
func (t *Tray) Run() error { return errors.New("系统托盘仅支持 Windows") }
