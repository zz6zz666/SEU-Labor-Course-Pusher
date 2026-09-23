/**
 * 解析层回归测试（离线可跑，不依赖网络与登录态）。
 *
 * 目的：PLAN.md 第 10 节列出的「必须原样保留的解析细节」一旦被改坏，
 * 在没有真实选课数据的情况下很难发现。这里用合成的、结构与站点一致的 HTML 夹具把
 * 列偏移判定、.limit-line 取值、过滤条件、去重键、过期清理全部钉死。
 *
 * 用法： npm run build && node tools/test-parse.js
 */
const assert = require('assert');
const {
  parseCourses,
  filterCourses,
  diffNewCourses,
  reconcileIds,
  hasCourseTable,
  looksLikeLoginPage,
  looksLikeCourseRoute,
  extractCourseTableHtml,
} = require('../dist/core/parse.js');

let passed = 0;
let failed = 0;

function check(name, fn) {
  try {
    fn();
    passed += 1;
    console.log(`  ok   ${name}`);
  } catch (err) {
    failed += 1;
    console.log(`  FAIL ${name}\n       ${err.message}`);
  }
}

// ---------------------------------------------------------------- 夹具构造

/** 构造一行 */
function row(cells, rowClass = 'c--tr') {
  return `<tr class="${rowClass}">${cells.map((c) => `<td>${c}</td>`).join('')}</tr>`;
}

/**
 * 按「含 offset 的列序」构造一行数据。
 * extraLeadingColumn=true 时在最前面插入一个非数字的附加列，
 * 用来验证原脚本的列偏移判定（第 1 列纯数字 → offset 0，否则 offset 1）。
 */
function rowBySpec(spec, opts = {}) {
  const offset = opts.extraLeadingColumn ? 1 : 0;
  const cols = new Array(16 + offset).fill('—');
  if (offset === 0) {
    cols[0] = spec.序号; // 纯数字
    cols[1] = '劳动教育';
  } else {
    cols[0] = ''; // 非数字的附加列 → 触发 offset=1
    cols[1] = spec.序号; // 数字
  }
  cols[3 + offset - 1] = spec.项目名称;
  cols[4 + offset - 1] = spec.项目类别;
  cols[7 + offset - 1] = spec.开课地点Html || `<div class="limit-line">${spec.开课地点}</div>`;
  cols[8 + offset - 1] = spec.实施时间;
  cols[9 + offset - 1] = spec.选课截止时间;
  cols[10 + offset - 1] = spec.选课人数;
  cols[15 + offset - 1] = spec.授课教师;
  return row(cols, opts.rowClass);
}

function table(rowsHtml) {
  return `<!DOCTYPE html><html><body><table id="c_app_page_index_XuanKe_table"><thead><tr><th>#</th></tr></thead><tbody>${rowsHtml}</tbody></table></body></html>`;
}

// ---------------------------------------------------------------- 夹具数据

const ROW_NORMAL = {
  序号: '1',
  项目名称: '烹饪实践',
  项目类别: '服务劳动',
  开课地点: '九龙湖校区 一食堂',
  实施时间: '2026-10-15 14:00-16:00',
  选课截止时间: '2026-10-10 23:59',
  选课人数: '32/40',
  授课教师: '张三',
};

const ROW_FULL = {
  ...ROW_NORMAL,
  序号: '2',
  项目名称: '校园修缮',
  项目类别: '生产劳动',
  开课地点: '四牌楼校区 体育馆',
  选课人数: '30/30 已满',
};

const ROW_EXPIRED = {
  ...ROW_NORMAL,
  序号: '3',
  项目名称: '图书馆整理',
  项目类别: '服务劳动',
  开课地点: '九龙湖校区 图书馆',
  选课截止时间: '2026-09-01 23:59 已截止',
};

const ROW_OFFSET1 = {
  ...ROW_NORMAL,
  序号: '4',
  项目名称: '实验室安全',
  项目类别: '服务劳动',
  开课地点: '九龙湖校区 实验楼',
  实施时间: '2026-11-02 09:00-11:00',
  授课教师: '李四',
};

function expectedWeekday(dateStr) {
  const m = dateStr.match(/\d{4}-\d{2}-\d{2}/);
  const d = new Date(m[0]);
  return ['周日', '周一', '周二', '周三', '周四', '周五', '周六'][d.getDay()];
}

// ---------------------------------------------------------------- 断言

console.log('\n[parse] 表格定位与页面分类');

check('能定位 id 为 c_app_page_index_XuanKe_table 的表格', () => {
  const html = table(rowBySpec(ROW_NORMAL));
  assert.ok(extractCourseTableHtml(html), '未取到表格内层 HTML');
  assert.strictEqual(hasCourseTable(html), true);
});

check('表格缺失时 tableFound = false', () => {
  const r = parseCourses('<html><body><p>没有表格</p></body></html>');
  assert.strictEqual(r.tableFound, false);
  assert.strictEqual(r.rowCount, 0);
});

check('能识别登录页特征', () => {
  assert.strictEqual(looksLikeLoginPage('<script>casLogin</script>'), true);
  assert.strictEqual(looksLikeLoginPage('<div>统一身份认证</div>'), true);
  assert.strictEqual(looksLikeLoginPage(table(rowBySpec(ROW_NORMAL))), false);
});

check('判定顺序保护：已登录页面即使页脚含 casLogin，正向信号仍然成立', () => {
  // 这是 probeAuth 里「先判表格、再判登录特征」的回归保护。
  // 若哪天把顺序倒过来，有效会话会被误判成失效，本用例会立刻失败。
  const html = table(rowBySpec(ROW_NORMAL)).replace(
    '</body>',
    '<a href="https://auth.seu.edu.cn/casback/casLogin">退出登录</a></body>',
  );
  assert.strictEqual(looksLikeLoginPage(html), true, '负向信号确实会命中（这正是需要保护的原因）');
  assert.strictEqual(hasCourseTable(html), true, '正向信号必须优先于负向信号');
});

console.log('\n[parse] 列偏移与字段提取');

check('第 1 列为纯数字 → offset = 0，字段落在正确列', () => {
  const r = parseCourses(table(rowBySpec(ROW_NORMAL)), { locations: [], categories: [] });
  assert.strictEqual(r.tableFound, true);
  assert.strictEqual(r.rowCount, 1);
  const c = r.courses[0];
  assert.strictEqual(c.序号, '1');
  assert.strictEqual(c.项目名称, '烹饪实践');
  assert.strictEqual(c.项目类别, '服务劳动');
  assert.strictEqual(c.开课地点, '九龙湖校区 一食堂');
  assert.strictEqual(c.选课截止时间, '2026-10-10 23:59');
  assert.strictEqual(c.选课人数_容纳人数, '32/40');
  assert.strictEqual(c.授课教师, '张三');
});

check('第 1 列非数字 → offset = 1，序号取自第 2 列', () => {
  const r = parseCourses(table(rowBySpec(ROW_OFFSET1, { extraLeadingColumn: true })), {
    locations: [],
    categories: [],
  });
  const c = r.courses[0];
  assert.strictEqual(c.序号, '4');
  assert.strictEqual(c.项目名称, '实验室安全');
  assert.strictEqual(c.开课地点, '九龙湖校区 实验楼');
  assert.strictEqual(c.授课教师, '李四');
  assert.strictEqual(c.选课人数_容纳人数, '32/40');
});

check('能从行首 td-data 提取 SJItemID / SJItemKaiKeID(自动选课依赖)', () => {
  const tdData =
    '<div class="hidden td-data">' +
    '<td-data data-name="ID" data-value="ROW-1"></td-data>' +
    '<td-data data-name="ItemName" data-value="烹饪实践"></td-data>' +
    '<td-data data-name="SJItemID" data-value="ITEM-1"></td-data>' +
    '<td-data data-name="SJItemKaiKeID" data-value="OPEN-1"></td-data>' +
    '</div>';
  const cells = new Array(16).fill('—');
  cells[0] = tdData + '1';
  cells[2] = '烹饪实践';
  cells[3] = '服务劳动';
  cells[6] = '<div class="limit-line">九龙湖校区 一食堂</div>';
  cells[7] = '2026-10-15 14:00-16:00';
  cells[8] = '2026-10-10 23:59';
  cells[9] = '32/40';
  cells[14] = '张三';
  const c = parseCourses(table(row(cells)), { locations: [], categories: [] }).courses[0];
  assert.strictEqual(c.sjItemId, 'ITEM-1');
  assert.strictEqual(c.sjItemKaiKeId, 'OPEN-1');
});

check('无 td-data 时 SJItemID 为空字符串(不误伤)', () => {
  const r = parseCourses(table(rowBySpec(ROW_NORMAL)), { locations: [], categories: [] });
  assert.strictEqual(r.courses[0].sjItemId, '');
  assert.strictEqual(r.courses[0].sjItemKaiKeId, '');
});

check('实施时间追加星期（与原脚本 getWeekday 一致）', () => {
  const r = parseCourses(table(rowBySpec(ROW_NORMAL)), { locations: [], categories: [] });
  const wd = expectedWeekday(ROW_NORMAL.实施时间);
  assert.strictEqual(r.courses[0].实施时间, `${ROW_NORMAL.实施时间}（${wd}）`);
});

check('开课地点取单元格内 .limit-line', () => {
  const r = parseCourses(table(rowBySpec(ROW_NORMAL)), { locations: [], categories: [] });
  assert.strictEqual(r.courses[0].开课地点, '九龙湖校区 一食堂');
});

check('去重键 = 项目名称|实施时间', () => {
  const r = parseCourses(table(rowBySpec(ROW_NORMAL)), { locations: [], categories: [] });
  assert.strictEqual(r.courses[0].uniqueId, `烹饪实践|${r.courses[0].实施时间}`);
});

check('兼容 .c-tr 行类名(服务端可能改名)', () => {
  const r = parseCourses(table(rowBySpec(ROW_NORMAL, { rowClass: 'c-tr' })));
  assert.strictEqual(r.rowCount, 1);
});

console.log('\n[parse] 过滤条件（未满员 + 未截止 + 筛选命中）');

check('已满 → isInvalid', () => {
  const r = parseCourses(table(rowBySpec(ROW_FULL)), { locations: [], categories: [] });
  assert.strictEqual(r.courses[0].isFull, true);
  assert.strictEqual(r.courses[0].isInvalid, true);
  assert.strictEqual(filterCourses(r.courses).length, 0);
});

check('已截止 → isInvalid', () => {
  const r = parseCourses(table(rowBySpec(ROW_EXPIRED)), { locations: [], categories: [] });
  assert.strictEqual(r.courses[0].isExpired, true);
  assert.strictEqual(filterCourses(r.courses).length, 0);
});

check('校区筛选：地点不匹配的被排除', () => {
  const html = table(rowBySpec(ROW_NORMAL) + rowBySpec(ROW_FULL));
  const r = parseCourses(html, { locations: ['九龙湖校区'], categories: [] });
  const valid = filterCourses(r.courses);
  assert.strictEqual(valid.length, 1);
  assert.strictEqual(valid[0].开课地点, '九龙湖校区 一食堂');
  // 四牌楼那条地点不匹配
  assert.strictEqual(r.courses[1].locationMatch, false);
});

check('地点为关键字包含匹配（如「无锡」命中「无锡校区食堂」）', () => {
  const html = table(rowBySpec({ ...ROW_NORMAL, 开课地点: '无锡校区食堂' }));
  const hit = parseCourses(html, { locations: ['无锡校区'], categories: [] });
  assert.strictEqual(hit.courses[0].locationMatch, true);
  const miss = parseCourses(html, { locations: ['九龙湖'], categories: [] });
  assert.strictEqual(miss.courses[0].locationMatch, false);
  // 多个关键字任一命中即可
  const any = parseCourses(html, { locations: ['九龙湖', '无锡'], categories: [] });
  assert.strictEqual(any.courses[0].locationMatch, true);
  // 空数组=不限制
  const all = parseCourses(html, { locations: [], categories: [] });
  assert.strictEqual(all.courses[0].locationMatch, true);
});

check('类别为黑名单：命中即排除，空数组不限制', () => {
  // ROW_NORMAL 类别=服务劳动，ROW_FULL 类别=生产劳动
  const html = table(rowBySpec(ROW_NORMAL) + rowBySpec(ROW_FULL));
  const r = parseCourses(html, { locations: [], categories: ['服务劳动'] });
  assert.strictEqual(r.courses[0].categoryAllowed, false, '服务劳动 应被排除');
  assert.strictEqual(r.courses[1].categoryAllowed, true, '生产劳动 应保留');
  // 黑名单为完全匹配，「服务」不应误伤「服务劳动」
  const r2 = parseCourses(html, { locations: [], categories: ['服务'] });
  assert.strictEqual(r2.courses[0].categoryAllowed, true);
  // 空数组=不限制
  const r3 = parseCourses(html, { locations: [], categories: [] });
  assert.strictEqual(r3.courses.every((c) => c.categoryAllowed), true);
});

check('筛选为空数组时不设限', () => {
  const html = table(rowBySpec(ROW_NORMAL) + rowBySpec(ROW_FULL) + rowBySpec(ROW_OFFSET1, { extraLeadingColumn: true }));
  const r = parseCourses(html, { locations: [], categories: [] });
  assert.strictEqual(filterCourses(r.courses).length, 2); // ROW_FULL 已满被排除
  assert.strictEqual(r.courses.every((c) => c.locationMatch && c.categoryAllowed), true);
});

console.log('\n[parse] 结构变化自救（.limit-line 全缺失时回退整格文本）');

check('全表无 .limit-line → 回退并使用整格文本，同时给出告警', () => {
  const html = table(rowBySpec({ ...ROW_NORMAL, 开课地点Html: '九龙湖校区 一食堂' }));
  const r = parseCourses(html, { locations: ['九龙湖校区'], categories: [] });
  assert.strictEqual(r.locationFallback, true);
  assert.ok(r.warnings.length > 0, '应给出告警');
  assert.strictEqual(r.courses[0].开课地点, '九龙湖校区 一食堂');
  assert.strictEqual(r.courses[0].locationMatch, true);
});

console.log('\n[parse] 差集与过期清理');

check('差集：已推送过的不再算新课程', () => {
  const r = parseCourses(table(rowBySpec(ROW_NORMAL) + rowBySpec(ROW_OFFSET1, { extraLeadingColumn: true })));
  const valid = filterCourses(r.courses);
  const pushed = new Set([valid[0].uniqueId]);
  const fresh = diffNewCourses(valid, pushed);
  assert.strictEqual(fresh.length, 1);
  assert.strictEqual(fresh[0].uniqueId, valid[1].uniqueId);
});

check('过期清理：消失的 / 已满的 / 不再匹配地点的记录都被移除', () => {
  const r = parseCourses(
    table(rowBySpec(ROW_NORMAL) + rowBySpec(ROW_FULL) + rowBySpec(ROW_EXPIRED)),
    { locations: ['九龙湖校区'], categories: [] },
  );
  const all = r.courses;
  const pushed = new Set([
    all[0].uniqueId, // 仍有效且匹配 → 保留
    all[1].uniqueId, // 已满 → 移除
    all[2].uniqueId, // 已截止 → 移除
    '早已不存在的课程|2026-01-01 00:00-02:00', // 已消失 → 移除
  ]);
  const kept = reconcileIds(all, pushed);
  assert.deepStrictEqual(kept, [all[0].uniqueId]);
});

check('HTML 实体与多余空白被正确清理', () => {
  const html = table(
    rowBySpec({ ...ROW_NORMAL, 项目名称: '  烹饪&amp;烘焙&nbsp;实践  ', 授课教师: '<span>张&nbsp;三</span>' }),
  );
  const r = parseCourses(html);
  assert.strictEqual(r.courses[0].项目名称, '烹饪&烘焙 实践');
  assert.strictEqual(r.courses[0].授课教师, '张 三');
});

check('属性值内含 ">" 不截断标签（真实站点 data-responsive--bind-click）', () => {
  // 站点真实写法：属性值里出现 `>`，用 [^>]* 匹配标签会在此截断，
  // 把 `span.c-link--line.c--view-SJItem">` 当成正文混进项目名称与去重键。
  const nameCell =
    `<span class="c-link--line c--view-SJItem" data-responsive--bind-click="td>span.c-link--line.c--view-SJItem">` +
    `\n  宿舍卫生劳动教育\n</span>\n` +
    `<span class="kt-badge kt-badge--inline">\n  预置排课\n</span>`;
  const html = table(rowBySpec({ ...ROW_NORMAL, 项目名称: nameCell }));
  const r = parseCourses(html);
  assert.strictEqual(r.courses[0].项目名称, '宿舍卫生劳动教育 预置排课');
  assert.ok(
    !r.courses[0].uniqueId.includes('c-link--line'),
    '去重键不应包含选择器碎片',
  );
});

// ---------------------------------------------------------------- 汇总

console.log(`\n结果：${passed} 通过 / ${failed} 失败\n`);
process.exit(failed === 0 ? 0 : 1);
