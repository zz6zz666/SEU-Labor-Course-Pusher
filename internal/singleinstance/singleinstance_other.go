//go:build !windows

package singleinstance

import "errors"

type Instance struct{}

func Acquire() (*Instance, bool, error) { return nil, false, nil }
func SignalExisting() error             { return errors.New("单实例锁仅支持 Windows") }
func (i *Instance) Wait() error         { return errors.New("单实例锁仅支持 Windows") }
func (i *Instance) Release()            {}
