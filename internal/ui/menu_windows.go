//go:build windows

package ui

import (
	"errors"
	"math"
	"unsafe"

	"golang.org/x/sys/windows"
)

// menuEntry is one row of the popup menu.
type menuEntry struct {
	label   string
	glyph   uint16
	sep     bool
	info    bool
	action  func()
	checked func() bool
}

var currentMenu *menu

type menu struct {
	tray    *Tray
	hwnd    windows.Handle
	entries []menuEntry
	hover   int
	dpi     int
	scale   float64

	textFont windows.Handle
	infoFont windows.Handle
	iconFont windows.Handle

	// logical metrics (scaled on use)
	padX, padY  int
	itemH, sepH int
	iconCol     int
	radius      int

	shownAt uint32
}

func newMenu(t *Tray) (*menu, error) {
	hInst := getModuleHandle()
	cls := utf16("seuLaborMenu")
	wc := wndClassExW{
		CbSize:        uint32(unsafe.Sizeof(wndClassExW{})),
		Style:         csDropShadow,
		HInstance:     hInst,
		HCursor:       loadArrowCursor(),
		LpfnWndProc:   windows.NewCallback(menuWndProc),
		LpszClassName: cls,
	}
	if atom, _, err := pRegisterClassExW.Call(uintptr(unsafe.Pointer(&wc))); atom == 0 {
		return nil, errors.Join(errors.New("创建菜单窗口类失败"), err)
	}

	exStyle := uintptr(wsExToolWindow | wsExTopmost | wsExLayered)
	hwnd, _, err := pCreateWindowExW.Call(
		exStyle,
		uintptr(unsafe.Pointer(cls)),
		uintptr(unsafe.Pointer(utf16("menu"))),
		wsPopup,
		0, 0, 0, 0,
		0, 0,
		uintptr(hInst),
		0,
	)
	if hwnd == 0 {
		return nil, errors.Join(errors.New("创建菜单窗口失败"), err)
	}

	m := &menu{tray: t, hwnd: windows.Handle(hwnd), hover: -1, scale: 1}
	setRoundedCorners(m.hwnd)
	currentMenu = m
	return m, nil
}

func (m *menu) buildEntries() {
	a := m.tray.actions
	m.entries = m.entries[:0]
	if a.StatusLines != nil {
		for _, line := range a.StatusLines() {
			if line == "" {
				continue
			}
			m.entries = append(m.entries, menuEntry{label: line, info: true})
		}
		if len(m.entries) > 0 {
			m.entries = append(m.entries, menuEntry{sep: true})
		}
	}
	m.entries = append(m.entries,
		menuEntry{label: "打开选课页", glyph: 0xE774, action: a.OnOpenCourse},
		menuEntry{label: "重新登录", glyph: 0xE72E, action: a.OnLogin},
		menuEntry{sep: true},
		menuEntry{label: "立即抓取一次", glyph: 0xE72C, action: a.OnFetchNow},
		menuEntry{label: "打开日志目录", glyph: 0xE8B7, action: a.OnOpenLogs},
		menuEntry{label: "打开 config.json", glyph: 0xE8A5, action: a.OnOpenConfig},
		menuEntry{label: "设置向导", glyph: 0xE713, action: a.OnOpenWizard},
		menuEntry{sep: true},
		menuEntry{label: "开机自启", action: a.ToggleAutoStart, checked: a.IsAutoStart},
		menuEntry{label: "自动选课", action: a.ToggleAutoSelect, checked: a.IsAutoSelect},
		menuEntry{sep: true},
	)
	if a.Version != "" {
		m.entries = append(m.entries, menuEntry{label: "版本 " + a.Version, info: true})
	}
	m.entries = append(m.entries, menuEntry{label: "退出", glyph: 0xE8BB, action: a.OnQuit})
}

func (m *menu) ensureDPI() {
	dpi := dpiForWindow(m.hwnd)
	if dpi == m.dpi && m.textFont != 0 {
		return
	}
	m.dpi = dpi
	m.scale = scaleFor(dpi)
	if m.textFont != 0 {
		pDeleteObject.Call(uintptr(m.textFont))
	}
	if m.infoFont != 0 {
		pDeleteObject.Call(uintptr(m.infoFont))
	}
	if m.iconFont != 0 {
		pDeleteObject.Call(uintptr(m.iconFont))
	}
	m.textFont = createFont("Segoe UI Variable Text", -int(math.Round(12*m.scale)), fwNormal)
	m.infoFont = createFont("Segoe UI Variable Text", -int(math.Round(11*m.scale)), fwNormal)
	m.iconFont = createFont("Segoe MDL2 Assets", -int(math.Round(14*m.scale)), fwNormal)

	m.padX = int(math.Round(14 * m.scale))
	m.padY = int(math.Round(6 * m.scale))
	m.itemH = int(math.Round(29 * m.scale))
	m.sepH = int(math.Round(10 * m.scale))
	m.iconCol = int(math.Round(31 * m.scale))
	m.radius = int(math.Round(6 * m.scale))
}

func (m *menu) measure() (int, int) {
	m.ensureDPI()
	hdc, _, _ := pGetDC.Call(uintptr(m.hwnd))
	defer pReleaseDC.Call(uintptr(m.hwnd), hdc)

	maxW := 0
	for _, e := range m.entries {
		if e.sep {
			continue
		}
		font := m.textFont
		if e.info {
			font = m.infoFont
		}
		_, _, _ = pSelectObject.Call(hdc, uintptr(font))
		w := textWidth(hdc, e.label)
		if w > maxW {
			maxW = w
		}
	}
	width := m.padX*2 + m.iconCol + maxW + int(math.Round(14*m.scale))
	if min := int(math.Round(210 * m.scale)); width < min {
		width = min
	}
	if max := int(math.Round(560 * m.scale)); width > max {
		width = max
	}

	height := m.padY * 2
	for _, e := range m.entries {
		if e.sep {
			height += m.sepH
		} else {
			height += m.itemH
		}
	}
	return width, height
}

func (m *menu) show() {
	m.buildEntries()
	w, h := m.measure()

	var pt point
	_, _, _ = pGetCursorPos.Call(uintptr(unsafe.Pointer(&pt)))
	x := int(pt.X) - w + int(math.Round(8*m.scale))
	y := int(pt.Y) - h - int(math.Round(8*m.scale))
	x, y = clampToWorkArea(x, y, w, h, pt)

	_, _, _ = pSetWindowPos.Call(
		uintptr(m.hwnd), 0,
		uintptr(x), uintptr(y), uintptr(w), uintptr(h),
		swpNoZOrder|swpNoActivate,
	)
	setRoundedCorners(m.hwnd)
	setBorderColor(m.hwnd, 0x00E4E4E4)
	// Uniform translucency (layered window): the desktop faintly shows through.
	setLayeredAlpha(m.hwnd, 240)
	m.hover = -1
	tick, _, _ := pGetTickCount.Call()
	m.shownAt = uint32(tick)

	_, _, _ = pShowWindow.Call(uintptr(m.hwnd), swShowNoActivate)
	_, _, _ = pSetForegroundWindow.Call(uintptr(m.hwnd))
	_, _, _ = pInvalidateRect.Call(uintptr(m.hwnd), 0, 1)
}

func (m *menu) hide() {
	if m.hwnd != 0 {
		_, _, _ = pShowWindow.Call(uintptr(m.hwnd), swHide)
	}
	// Returning focus to the hidden owner keeps the shell happy.
	_, _, _ = pSetForegroundWindow.Call(uintptr(m.tray.hwnd))
}

func (m *menu) activate(i int) {
	if i < 0 || i >= len(m.entries) {
		return
	}
	e := m.entries[i]
	if e.sep || e.info || e.action == nil {
		return
	}
	fn := e.action
	// Toggles keep the menu open so the check updates in place and another
	// option can be flipped immediately; a click elsewhere dismisses it.
	if e.checked != nil {
		go func() {
			fn()
			_, _, _ = pInvalidateRect.Call(uintptr(m.hwnd), 0, 1)
		}()
		return
	}
	m.hide()
	go fn()
}

// --- drawing ---

func (m *menu) onPaint() {
	var ps struct {
		Hdc         windows.Handle
		FErase      int32
		RcPaint     rect
		FRestore    int32
		FIncUpdate  int32
		RgbReserved [32]byte
	}
	hdc, _, _ := pBeginPaint.Call(uintptr(m.hwnd), uintptr(unsafe.Pointer(&ps)))
	if hdc == 0 {
		return
	}
	defer pEndPaint.Call(uintptr(m.hwnd), uintptr(unsafe.Pointer(&ps)))

	var cr rect
	_, _, _ = pGetClientRect.Call(uintptr(m.hwnd), uintptr(unsafe.Pointer(&cr)))
	w := int(cr.Right - cr.Left)
	h := int(cr.Bottom - cr.Top)
	if w <= 0 || h <= 0 {
		return
	}

	mem, _, _ := pCreateCompatibleDC.Call(hdc)
	bmp, _, _ := pCreateCompatibleBitmap.Call(hdc, uintptr(w), uintptr(h))
	old := selectObjectHandle(mem, bmp)
	m.paintInto(mem, w, h)
	_, _, _ = pBitBlt.Call(hdc, 0, 0, uintptr(w), uintptr(h), mem, 0, 0, srcCopy)
	selectObjectHandle(mem, old)
	pDeleteObject.Call(bmp)
	pDeleteDC.Call(mem)
}

func (m *menu) paintInto(hdc uintptr, w, h int) {
	var full rect
	full.Right = int32(w)
	full.Bottom = int32(h)
	bg := solidBrush(0xF9F9F9)
	fillRect(hdc, &full, bg)
	pDeleteObject.Call(bg)

	_, _, _ = pSetBkMode.Call(hdc, transparent)

	y := int32(m.padY)
	for i, e := range m.entries {
		if e.sep {
			line := rect{
				Left:   int32(m.padX + int(math.Round(10*m.scale))),
				Right:  int32(w - m.padX - int(math.Round(10*m.scale))),
				Top:    y + int32(m.sepH/2),
				Bottom: y + int32(m.sepH/2) + 1,
			}
			sep := solidBrush(0xE4E4E4)
			fillRect(hdc, &line, sep)
			pDeleteObject.Call(sep)
			y += int32(m.sepH)
			continue
		}

		row := rect{Left: int32(m.padX), Top: y, Right: int32(w - m.padX), Bottom: y + int32(m.itemH)}
		if e.info {
			label := rect{Left: row.Left + int32(m.iconCol), Top: row.Top, Right: row.Right, Bottom: row.Bottom}
			_, _, _ = pSelectObject.Call(hdc, uintptr(m.infoFont))
			_, _, _ = pSetTextColor.Call(hdc, 0x00808080)
			drawText(hdc, e.label, &label, dtLeft|dtVCenter|dtSingleLine|dtNoPrefix)
			y += int32(m.itemH)
			continue
		}
		if i == m.hover {
			hb := solidBrush(0xEBEBEB)
			pen, _, _ := pCreatePen.Call(psSolid, 1, 0xEBEBEB)
			oldPen := selectObjectHandle(hdc, pen)
			oldBrush := selectObjectHandle(hdc, hb)
			pRoundRect.Call(hdc, uintptr(row.Left), uintptr(row.Top), uintptr(row.Right), uintptr(row.Bottom), uintptr(m.radius*2), uintptr(m.radius*2))
			selectObjectHandle(hdc, oldPen)
			selectObjectHandle(hdc, oldBrush)
			pDeleteObject.Call(pen)
			pDeleteObject.Call(hb)
		}

		// leading gutter: a check for toggles, an icon otherwise
		gutter := rect{Left: row.Left, Top: row.Top, Right: row.Left + int32(m.iconCol), Bottom: row.Bottom}
		if e.checked != nil {
			if e.checked() {
				_, _, _ = pSelectObject.Call(hdc, uintptr(m.iconFont))
				_, _, _ = pSetTextColor.Call(hdc, 0x00EB6F2F) // #2F6FEB
				drawText(hdc, string(rune(0xE73E)), &gutter, dtCenter|dtVCenter|dtSingleLine|dtNoPrefix)
			}
		} else if e.glyph != 0 {
			_, _, _ = pSelectObject.Call(hdc, uintptr(m.iconFont))
			_, _, _ = pSetTextColor.Call(hdc, 0x00666666)
			drawText(hdc, string(rune(e.glyph)), &gutter, dtCenter|dtVCenter|dtSingleLine|dtNoPrefix)
		}

		// label
		label := rect{
			Left:   row.Left + int32(m.iconCol),
			Top:    row.Top,
			Right:  row.Right,
			Bottom: row.Bottom,
		}
		_, _, _ = pSelectObject.Call(hdc, uintptr(m.textFont))
		_, _, _ = pSetTextColor.Call(hdc, 0x001A1A1A)
		drawText(hdc, e.label, &label, dtLeft|dtVCenter|dtSingleLine|dtNoPrefix)

		y += int32(m.itemH)
	}
}

func (m *menu) hitTest(x, y int32) int {
	if x < int32(m.padX) {
		return -1
	}
	cy := int32(m.padY)
	for i, e := range m.entries {
		hh := int32(m.itemH)
		if e.sep {
			hh = int32(m.sepH)
		}
		if y >= cy && y < cy+hh {
			if e.sep || e.info {
				return -1
			}
			return i
		}
		cy += hh
	}
	return -1
}

func (m *menu) moveHover(delta int) {
	if len(m.entries) == 0 {
		return
	}
	i := m.hover
	for k := 0; k < len(m.entries); k++ {
		i += delta
		if i < 0 {
			i = len(m.entries) - 1
		}
		if i >= len(m.entries) {
			i = 0
		}
		if !m.entries[i].sep && !m.entries[i].info {
			break
		}
	}
	m.hover = i
	_, _, _ = pInvalidateRect.Call(uintptr(m.hwnd), 0, 1)
}

func menuWndProc(hwnd uintptr, msg uint32, wp, lp uintptr) uintptr {
	m := currentMenu
	if m == nil || m.hwnd != windows.Handle(hwnd) {
		r, _, _ := pDefWindowProcW.Call(hwnd, uintptr(msg), wp, lp)
		return r
	}
	switch msg {
	case wmPaint:
		m.onPaint()
		return 0
	case wmEraseBkgnd:
		return 1
	case wmSetCursor:
		if uint32(lp&0xFFFF) == htClient {
			setArrowCursor()
			return 1
		}
	case wmMouseMove:
		x := int32(int16(lp & 0xFFFF))
		y := int32(int16((lp >> 16) & 0xFFFF))
		h := m.hitTest(x, y)
		if h != m.hover {
			m.hover = h
			_, _, _ = pInvalidateRect.Call(uintptr(hwnd), 0, 1)
		}
		var tme trackMouseEvent
		tme.CbSize = uint32(unsafe.Sizeof(trackMouseEvent{}))
		tme.DwFlags = tmeLeave
		tme.HwndTrack = windows.Handle(hwnd)
		pTrackMouseEvent.Call(uintptr(unsafe.Pointer(&tme)))
		return 0
	case wmMouseLeave:
		if m.hover != -1 {
			m.hover = -1
			_, _, _ = pInvalidateRect.Call(uintptr(hwnd), 0, 1)
		}
		return 0
	case wmLButtonUp:
		m.activate(m.hover)
		return 0
	case wmKeyDown:
		switch wp {
		case 0x26: // VK_UP
			m.moveHover(-1)
		case 0x28: // VK_DOWN
			m.moveHover(1)
		case 0x0D, 0x20: // ENTER, SPACE
			m.activate(m.hover)
		case 0x1B: // ESC
			m.hide()
		}
		return 0
	case wmActivate:
		if uint16(wp&0xFFFF) == waInactive {
			tick, _, _ := pGetTickCount.Call()
			if uint32(tick)-m.shownAt > 250 {
				m.hide()
			}
		}
		return 0
	case wmKillFocus:
		return 0
	case wmClose:
		_, _, _ = pShowWindow.Call(uintptr(hwnd), swHide)
		return 0
	}
	r, _, _ := pDefWindowProcW.Call(hwnd, uintptr(msg), wp, lp)
	return r
}

// --- small GDI helpers ---

func createFont(face string, height, weight int) windows.Handle {
	h, _, _ := pCreateFontW.Call(
		uintptr(int32(height)),
		0, 0, 0,
		uintptr(weight),
		0, 0, 0,
		1, // DEFAULT_CHARSET
		0, 0,
		5, // CLEARTYPE_QUALITY
		0,
		uintptr(unsafe.Pointer(utf16(face))),
	)
	return windows.Handle(h)
}

func solidBrush(color uint32) uintptr {
	h, _, _ := pCreateSolidBrush.Call(uintptr(color))
	return h
}

func fillRect(hdc uintptr, r *rect, brush uintptr) {
	_, _, _ = pFillRect.Call(hdc, uintptr(unsafe.Pointer(r)), brush)
}

func selectObjectHandle(hdc, obj uintptr) uintptr {
	r, _, _ := pSelectObject.Call(hdc, obj)
	return r
}

func drawText(hdc uintptr, s string, r *rect, format uint32) {
	u, _ := windows.UTF16FromString(s)
	_, _, _ = pDrawTextW.Call(hdc, uintptr(unsafe.Pointer(&u[0])), ^uintptr(0), uintptr(unsafe.Pointer(r)), uintptr(format))
}

func textWidth(hdc uintptr, s string) int {
	u, _ := windows.UTF16FromString(s)
	var sz struct{ Cx, Cy int32 }
	_, _, _ = pGetTextExtentPoint32W.Call(hdc, uintptr(unsafe.Pointer(&u[0])), uintptr(len(u)-1), uintptr(unsafe.Pointer(&sz)))
	return int(sz.Cx)
}
