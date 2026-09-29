package site

import (
	"regexp"
	"strings"
	"time"

	"golang.org/x/net/html"
)

const courseTableID = "c_app_page_index_XuanKe_table"

var (
	whitespace  = regexp.MustCompile(`\s+`)
	datePattern = regexp.MustCompile(`\d{4}-\d{2}-\d{2}`)
	weekdays    = [...]string{"周日", "周一", "周二", "周三", "周四", "周五", "周六"}
)

// Parse extracts courses from the raw course-page HTML.
//
// Column layout (1-based, offset accounts for an optional leading index
// column): name 3, category 4, location 7 (.limit-line), time 8, deadline 9,
// enrollment 10, teacher 15.
func Parse(pageHTML string, opts FilterOptions) ParseResult {
	doc, err := html.Parse(strings.NewReader(pageHTML))
	if err != nil {
		return ParseResult{}
	}
	table := findElement(doc, func(n *html.Node) bool {
		return n.Data == "table" && attr(n, "id") == courseTableID
	})
	if table == nil {
		return ParseResult{}
	}

	rows := courseRows(table)
	result := ParseResult{TableFound: true, RowCount: len(rows)}
	for _, row := range rows {
		cells := directChildren(row, "td")
		if len(cells) < 4 {
			continue
		}
		result.Courses = append(result.Courses, buildCourse(cells, opts))
	}
	return result
}

func buildCourse(cells []*html.Node, opts FilterOptions) Course {
	cellText := func(oneBased int) string {
		if oneBased < 1 || oneBased > len(cells) {
			return "无"
		}
		return cleanText(textOf(cells[oneBased-1]))
	}

	col1 := cellText(1)
	col2 := cellText(2)
	offset := 1
	if isPureNumber(col1) && !isPureNumber(col2) {
		offset = 0
	}

	name := cellText(3 + offset)
	time := cellText(8 + offset)
	deadline := cellText(9 + offset)
	enrollment := cellText(10 + offset)

	locCell := findInCell(cells, 7+offset)
	location := "无"
	if locCell != nil {
		if line := findElement(locCell, hasClass("limit-line")); line != nil {
			location = cleanText(textOf(line))
		}
	}

	seq := col1
	if offset == 1 {
		seq = col2
	}

	full := strings.Contains(enrollment, "已满")
	expired := strings.Contains(deadline, "已截止")
	tdData := tdDataMap(cells[0])

	return Course{
		Seq:        seq,
		Name:       name,
		Category:   cellText(4 + offset),
		Location:   location,
		Time:       appendWeekday(time),
		Deadline:   deadline,
		Enrollment: enrollment,
		Teacher:    cellText(15 + offset),
		Invalid:    full || expired,
		Full:       full,
		Expired:    expired,
		LocationOK: opts.matchLocation(location),
		CategoryOK: opts.matchCategory(cellText(4 + offset)),
		ItemID:     tdData["SJItemID"],
		KaiKeID:    tdData["SJItemKaiKeID"],
	}
}

func findInCell(cells []*html.Node, oneBased int) *html.Node {
	if oneBased < 1 || oneBased > len(cells) {
		return nil
	}
	return cells[oneBased-1]
}

func appendWeekday(s string) string {
	m := datePattern.FindString(s)
	if m == "" {
		return s
	}
	t, err := time.Parse("2006-01-02", m)
	if err != nil {
		return s
	}
	return s + "（" + weekdays[t.Weekday()] + "）"
}

// --- html helpers ---

func courseRows(table *html.Node) []*html.Node {
	tbody := findElement(table, func(n *html.Node) bool { return n.Data == "tbody" })
	if tbody == nil {
		return nil
	}
	var rows []*html.Node
	for c := tbody.FirstChild; c != nil; c = c.NextSibling {
		if c.Type == html.ElementNode && c.Data == "tr" && (hasClassValue(c, "c--tr") || hasClassValue(c, "c-tr")) {
			rows = append(rows, c)
		}
	}
	return rows
}

func directChildren(n *html.Node, tag string) []*html.Node {
	var out []*html.Node
	for c := n.FirstChild; c != nil; c = c.NextSibling {
		if c.Type == html.ElementNode && c.Data == tag {
			out = append(out, c)
		}
	}
	return out
}

func findElement(root *html.Node, pred func(*html.Node) bool) *html.Node {
	if root.Type == html.ElementNode && pred(root) {
		return root
	}
	for c := root.FirstChild; c != nil; c = c.NextSibling {
		if found := findElement(c, pred); found != nil {
			return found
		}
	}
	return nil
}

func tdDataMap(cell *html.Node) map[string]string {
	out := map[string]string{}
	if cell == nil {
		return out
	}
	var walk func(*html.Node)
	walk = func(n *html.Node) {
		if n.Type == html.ElementNode && n.Data == "td-data" {
			if name := attr(n, "data-name"); name != "" {
				out[name] = attr(n, "data-value")
			}
		}
		for c := n.FirstChild; c != nil; c = c.NextSibling {
			walk(c)
		}
	}
	walk(cell)
	return out
}

func textOf(n *html.Node) string {
	var b strings.Builder
	var walk func(*html.Node)
	walk = func(node *html.Node) {
		if node.Type == html.TextNode {
			b.WriteString(node.Data)
		}
		for c := node.FirstChild; c != nil; c = c.NextSibling {
			walk(c)
		}
	}
	walk(n)
	return b.String()
}

func cleanText(s string) string {
	s = strings.TrimSpace(whitespace.ReplaceAllString(strings.ReplaceAll(s, "\u00a0", " "), " "))
	if s == "" {
		return "无"
	}
	return s
}

func attr(n *html.Node, key string) string {
	for _, a := range n.Attr {
		if a.Key == key {
			return a.Val
		}
	}
	return ""
}

func hasClassValue(n *html.Node, class string) bool {
	for _, f := range strings.Fields(attr(n, "class")) {
		if f == class {
			return true
		}
	}
	return false
}

func hasClass(class string) func(*html.Node) bool {
	return func(n *html.Node) bool { return hasClassValue(n, class) }
}
