package site

import "testing"

const page = `<!DOCTYPE html><html><body>
<table id="c_app_page_index_XuanKe_table">
<tbody>
<tr class="c--tr">
  <td>1<div class="hidden td-data"><td-data data-name="SJItemID" data-value="111"></td-data><td-data data-name="SJItemKaiKeID" data-value="222"></td-data></div></td>
  <td>--</td>
  <td>项目A</td>
  <td>生活劳动</td>
  <td>x</td>
  <td>x</td>
  <td><span class="limit-line">九龙湖校区</span></td>
  <td>2026-10-01 08:00-10:00</td>
  <td>未截止</td>
  <td>10/30</td>
  <td>x</td><td>x</td><td>x</td><td>x</td>
  <td>教师A</td>
</tr>
<tr class="c--tr">
  <td>2</td><td>--</td>
  <td>项目B</td>
  <td>服务劳动</td>
  <td>x</td><td>x</td>
  <td><span class="limit-line">四牌楼校区</span></td>
  <td>2026-10-02 08:00-10:00</td>
  <td>已截止</td>
  <td>30/30</td>
  <td>x</td><td>x</td><td>x</td><td>x</td>
  <td>教师B</td>
</tr>
</tbody>
</table>
</body></html>`

func TestParseCourses(t *testing.T) {
	result := Parse(page, FilterOptions{Locations: []string{"九龙湖"}})

	if !result.TableFound {
		t.Fatal("table not found")
	}
	if result.RowCount != 2 {
		t.Fatalf("RowCount = %d, want 2", result.RowCount)
	}
	if len(result.Courses) != 2 {
		t.Fatalf("courses = %d, want 2", len(result.Courses))
	}

	a := result.Courses[0]
	if a.Name != "项目A" || a.Category != "生活劳动" {
		t.Errorf("course A = %q/%q", a.Name, a.Category)
	}
	if a.Location != "九龙湖校区" {
		t.Errorf("location = %q, want 九龙湖校区", a.Location)
	}
	if a.ItemID != "111" || a.KaiKeID != "222" {
		t.Errorf("ids = %q/%q, want 111/222", a.ItemID, a.KaiKeID)
	}
	if a.Full || a.Expired || a.Invalid {
		t.Errorf("course A should be valid, got full=%v expired=%v", a.Full, a.Expired)
	}
	if !a.LocationOK {
		t.Error("course A should match location filter")
	}
	if a.UniqueID() != "项目A|2026-10-01 08:00-10:00（周四）" {
		t.Errorf("uniqueID = %q", a.UniqueID())
	}

	b := result.Courses[1]
	if !b.Expired || !b.Invalid {
		t.Errorf("course B should be expired/invalid, got %v/%v", b.Expired, b.Invalid)
	}
	if b.LocationOK {
		t.Error("course B should not match 九龙湖 location filter")
	}
	if got := len(DiffNew(result.Courses, map[string]struct{}{})); got != 1 {
		t.Errorf("DiffNew eligible = %d, want 1", got)
	}
}

func TestCategoryBlacklist(t *testing.T) {
	result := Parse(page, FilterOptions{Categories: []string{"生活劳动"}})
	if result.Courses[0].CategoryOK {
		t.Error("生活劳动 should be excluded by blacklist")
	}
	if !result.Courses[1].CategoryOK {
		t.Error("服务劳动 should be allowed")
	}
}
