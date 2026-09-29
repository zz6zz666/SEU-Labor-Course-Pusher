// Package site turns the SEU labor course page into structured data.
//
// The list is server-rendered (verified against a live session): a plain HTTP
// GET returns the table with data rows, so no browser engine is needed to poll.
package site

import (
	"regexp"
	"strings"
)

type Course struct {
	Seq        string
	Name       string
	Category   string
	Location   string
	Time       string
	Deadline   string
	Enrollment string
	Teacher    string
	Invalid    bool
	Full       bool
	Expired    bool
	LocationOK bool
	CategoryOK bool
	ItemID     string
	KaiKeID    string
}

func (c Course) UniqueID() string { return c.Name + "|" + c.Time }

type ParseResult struct {
	TableFound bool
	RowCount   int
	Courses    []Course
}

// FilterOptions mirror the config semantics: locations is a substring
// whitelist, categories is an exact-match blacklist.
type FilterOptions struct {
	Locations  []string
	Categories []string
}

func (o FilterOptions) matchLocation(loc string) bool {
	if len(o.Locations) == 0 {
		return true
	}
	for _, kw := range o.Locations {
		if strings.Contains(loc, kw) {
			return true
		}
	}
	return false
}

func (o FilterOptions) matchCategory(cat string) bool {
	for _, black := range o.Categories {
		if cat == black {
			return false
		}
	}
	return true
}

// Eligible reports whether a course should be pushed.
func (c Course) Eligible() bool {
	return c.LocationOK && c.CategoryOK && !c.Invalid
}

// DiffNew returns eligible courses whose unique id is not already tracked.
func DiffNew(courses []Course, pushed map[string]struct{}) []Course {
	var out []Course
	for _, c := range courses {
		if c.Eligible() {
			if _, seen := pushed[c.UniqueID()]; !seen {
				out = append(out, c)
			}
		}
	}
	return out
}

// ReconcilePushed drops tracked ids that disappeared, became invalid or no
// longer match the filters.
func ReconcilePushed(courses []Course, pushed []string) []string {
	byID := make(map[string]Course, len(courses))
	for _, c := range courses {
		byID[c.UniqueID()] = c
	}
	var out []string
	for _, id := range pushed {
		c, ok := byID[id]
		if !ok || !c.Eligible() {
			continue
		}
		out = append(out, id)
	}
	return out
}

var pureNumber = regexp.MustCompile(`^\d+$`)

func isPureNumber(s string) bool { return pureNumber.MatchString(strings.TrimSpace(s)) }
