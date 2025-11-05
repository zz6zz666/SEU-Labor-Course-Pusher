// ==UserScript==
// @name         SEU劳动教育课程推送助手
// @namespace    http://tampermonkey.net/
// @version      1.0
// @license      MIT
// @description  东南大学劳动教育选课神器！实时监控新增课程并微信推送，打开浏览器就会后台自动运行，无需频繁登录查询即可获取所在校区的最新劳动教育实践课程信息
// @author       zz6zz666@github with AI support
// @match        *://*/*
// @grant        GM_xmlhttpRequest
// @grant        GM_setValue
// @grant        GM_getValue
// @grant        GM_openInTab
// @run-at       document-end
// ==/UserScript==

(function () {
    'use strict';

    // ==================== 全局配置区（用户可自定义）====================
    // 【后台页面活性维持配置】
    const COOLDOWN = 180 * 1000;        // 冷却时间（单位：毫秒）- 非目标页面创建新标签页的最小时间间隔
    const HEARTBEAT_INTERVAL = 20 * 1000;   // 报活间隔 - 通过目标页面的持续报活来防止重复创建标签页
    const CHECK_INTERVAL = 60 * 1000;       // 检查间隔 - 非目标页面定时检查是否需要创建新标签页

    // 【登录与推送配置】
    const USERNAME = '12345678';       // 替换为你的一卡通号
    const PASSWORD = 'abc123456';      // 替换为你的密码
    const PUSHPLUS_TOKEN = 'ce0**********************************11'; // 替换为你的PushPlus Token
    const PUSH_TITLE = '劳动教育课程推送'; // 微信推送标题
    const LOCATION_FILTERS = [];       // 校区筛选，如['四牌楼校区', '九龙湖校区']，为空则不筛选
    const REFRESH_INTERVAL = 3 * 60 * 1000; // 选课页自动刷新间隔（单位：毫秒）
    const LOGIN_TIMEOUT = 10 * 1000;  // 登录超时检测时间（单位：毫秒）
    const LOGIN_DISABLE_DURATION = 30 * 60 * 1000; // 登录失败后禁用自动登录时长（单位：毫秒）
    // =================================================================

    // ==================== 工具函数 ====================
    /**
     * 发送微信推送（支持自定义标题）
     * @param {string} title - 推送标题
     * @param {string} content - 推送内容（Markdown格式）
     */
    function pushToWechat(title, content) {
        if (!PUSHPLUS_TOKEN) {
            console.error('【错误】请填写PushPlus Token');
            return;
        }
        GM_xmlhttpRequest({
            method: "POST",
            url: 'http://www.pushplus.plus/send',
            headers: {'Content-Type': 'application/json'},
            data: JSON.stringify({
                'token': PUSHPLUS_TOKEN,
                'title': title,
                'content': content,
                'template': 'markdown'
            }),
            onload: function(response) {
                console.log('%c【推送成功】', 'color:green; font-weight:bold;');
                console.log(response.responseText);
            },
            onerror: function(error) {
                console.error('【推送失败】', error);
            }
        });
    }

    /**
     * 检查是否需要禁用自动登录
     * @returns {boolean} - 是否在登录失败冷却期内
     */
    function shouldDisableLogin() {
        const lastFailTime = GM_getValue('loginFailTime', 0);
        const now = Date.now();
        return lastFailTime > 0 && (now - lastFailTime) < LOGIN_DISABLE_DURATION;
    }
    // =================================================

    // ==================== 后台页面活性维持逻辑 ====================
    const currentUrl = window.location.href;
    const LOGIN_LABOR_URL = "https://auth.seu.edu.cn/dist/#/dist/main/login?service=https://labor.seu.edu.cn/UnifiedAuth/CASLogin";

    /**
     * 目标页面报活逻辑（持续更新最后活跃时间）
     */
    function handleTargetPage() {
        function updateLastActive() {
            const now = Date.now();
            GM_setValue('lastTargetActive', now);
            console.log(`[页面活性] 目标页面报活，时间：${new Date(now).toLocaleTimeString()}`);
        }
        updateLastActive();
        setInterval(updateLastActive, HEARTBEAT_INTERVAL);
    }

    /**
     * 非目标页面检查逻辑（定时判断是否创建新标签页）
     */
    function handleNonTargetPage() {
        function checkAndCreate() {
            const lastActive = GM_getValue('lastTargetActive', 0);
            const now = Date.now();
            const timeSinceLast = now - lastActive;

            if (timeSinceLast > COOLDOWN) {
                console.log(`[页面活性] 超过${COOLDOWN/1000}秒无目标页面活跃，创建新标签页`);
                GM_openInTab(LOGIN_LABOR_URL, {
                    active: false,
                    insert: true
                });
                GM_setValue('lastTargetActive', now);
            } else {
                console.log(`[页面活性] 目标页面活跃，剩余冷却：${Math.floor((COOLDOWN - timeSinceLast)/1000)}秒`);
            }
        }
        checkAndCreate();
        setInterval(checkAndCreate, CHECK_INTERVAL);
    }

    if (currentUrl.includes('labor.seu.edu.cn') || currentUrl.includes('auth.seu.edu.cn/dist')) {
        handleTargetPage();
    } else {
        handleNonTargetPage();
    }
    // ==============================================================

    // ==================== 自动登录与选课跳转逻辑 ====================
    /**
     * 登录页自动登录处理
     */
    function handleLoginPage() {
        if (shouldDisableLogin()) {
            const lastFailTime = new Date(GM_getValue('loginFailTime', 0)).toLocaleString();
            console.log(`[自动登录] 登录失败后禁用期内（上次失败时间：${lastFailTime}），暂不执行自动登录`);
            return;
        }

        console.log('[自动登录] 检测到登录页，开始自动登录...');

        let loginSuccess = false;
        const loginTimer = setTimeout(() => {
            if (!loginSuccess) {
                console.error('[自动登录] 登录超时，未检测到跳转');
                GM_setValue('loginFailStatus', true);
                GM_setValue('loginFailTime', Date.now());
                pushToWechat('课程推送登录失效提醒',
            `## 统一身份认证登录超时\n\n⚠️ 登录尝试超过${LOGIN_TIMEOUT/1000}秒未跳转，可能是以下原因：\n1. 需要短信验证码\n2. 账号密码错误\n3. 系统临时故障\n\n请手动登录检查状态\n时间：${new Date().toLocaleString()}`);
            }
        }, LOGIN_TIMEOUT);

        // 监听页面跳转判断登录成功
        const originalPushState = history.pushState;
        history.pushState = function(...args) {
            loginSuccess = true;
            clearTimeout(loginTimer);
            return originalPushState.apply(this, args);
        };

        window.addEventListener('beforeunload', () => {
            loginSuccess = true;
            clearTimeout(loginTimer);
        });

        // 等待登录元素加载
        const waitForElements = () => {
            const usernameInput = document.querySelector('input.input-username-pc[type="text"]') ||
                                  document.querySelector('input[type="text"][placeholder*="学号"]');

            const passwordInput = document.querySelector('input[type="password"]') ||
                                  document.querySelector('input.input-password-pc');

            const loginButton = document.querySelector('button.login-button-pc') ||
                                document.querySelector('button[type="button"]');

            if (!usernameInput || !passwordInput || !loginButton) {
                console.log('[自动登录] 元素未找到，500ms 后重试...');
                setTimeout(waitForElements, 500);
                return;
            }

            console.log('[自动登录] 找到输入框，等待 3 秒后输入信息...');

            setTimeout(() => {
                // 强制设置输入值（兼容React等框架）
                const forceSetValue = (input, value) => {
                    const lastValue = input.value;
                    input.value = value;
                    const event = new Event('input', { bubbles: true });
                    event.simulated = true;
                    const tracker = input._valueTracker;
                    if (tracker) tracker.setValue(lastValue);
                    input.dispatchEvent(event);
                };

                forceSetValue(usernameInput, USERNAME);
                forceSetValue(passwordInput, PASSWORD);

                setTimeout(() => {
                    if (!loginButton.disabled) {
                        console.log('[自动登录] 点击登录按钮...');
                        loginButton.click();
                    } else {
                        console.log('[自动登录] 登录按钮禁用，1秒后重试点击...');
                        setTimeout(() => loginButton.click(), 1000);
                    }
                }, 500);
            }, 3000);
        };

        setTimeout(waitForElements, 1000);
    }

    /**
     * 劳动教育首页跳转选课页
     */
    function handleLaborHomePage() {
        console.log('[页面跳转] 检测到劳动教育首页，准备跳转选课页...');

        // 清除登录失败状态
        GM_setValue('loginFailStatus', false);
        GM_setValue('loginFailTime', 0);

        const targetUrl = 'https://labor.seu.edu.cn/SJItemKaiKe/XuanKe/Index';
        setTimeout(() => {
            console.log('[页面跳转] 正在跳转到选课页:', targetUrl);
            window.location.href = targetUrl;
        }, 1000);
    }
    // ==============================================================

    // ==================== 课程监控与推送逻辑 ====================
    /**
     * 获取日期对应的星期
     * @param {string} dateStr - 日期字符串
     * @returns {string} - 星期（如“周二”）
     */
    function getWeekday(dateStr) {
        if (!dateStr) return '';
        const dateMatch = dateStr.match(/\d{4}-\d{2}-\d{2}/);
        if (!dateMatch) return '';
        const date = new Date(dateMatch[0]);
        return isNaN(date.getTime()) ? '' : ['周日', '周一', '周二', '周三', '周四', '周五', '周六'][date.getDay()];
    }

    /**
     * 清除文本多余空格/换行
     * @param {string} text - 原始文本
     * @returns {string} - 清理后的文本
     */
    const cleanText = (text) => text ? text.trim().replace(/\s+/g, ' ') : '无';

    /**
     * 提取课程信息（含唯一标识）
     * @param {Element} row - 课程行元素
     * @returns {Object} - 课程信息对象
     */
    function extractCourseInfo(row) {
        // 辅助函数：判断文本是否为纯数字
        const isPureNumber = (text) => /^\d+$/.test(text.trim());

        // 获取第1列和第2列文本，判断序号所在列
        const col1Text = cleanText(row.querySelector('td:nth-child(1)')?.textContent || '');
        const col2Text = cleanText(row.querySelector('td:nth-child(2)')?.textContent || '');
        const isIndexInCol1 = isPureNumber(col1Text) && !isPureNumber(col2Text);
        const offset = isIndexInCol1 ? 0 : 1; // 序号在第1列时所有列索引减1（向前挪一位）

        // 根据偏移计算实际列索引
        const originalTime = cleanText(row.querySelector(`td:nth-child(${8 + offset})`).textContent);
        const weekday = getWeekday(originalTime);
        const 选课状态 = cleanText(row.querySelector(`td:nth-child(${10 + offset})`).textContent);
        const 截止状态 = cleanText(row.querySelector(`td:nth-child(${9 + offset})`).textContent);
        const 开课地点 = cleanText(row.querySelector(`td:nth-child(${7 + offset}) .limit-line`)?.textContent);
        const 项目名称 = cleanText(row.querySelector(`td:nth-child(${3 + offset})`).textContent);
        const 实施时间 = weekday ? `${originalTime}（${weekday}）` : originalTime;

        // 核心：使用「项目名称+实施时间」作为唯一标识（避免重名）
        const uniqueId = `${项目名称}|${实施时间}`;

        // 判断是否已满或已截止
        const isFull = 选课状态.includes('已满');
        const isExpired = 截止状态.includes('已截止');
        const isInvalid = isFull || isExpired;

        // 判断地点是否符合配置
        const locationMatch = LOCATION_FILTERS.length === 0
        ? true
        : LOCATION_FILTERS.some(filter => 开课地点?.includes(filter));

        return {
            uniqueId,
            序号: isIndexInCol1 ? col1Text : col2Text, // 序号取纯数字所在列
            项目名称,
            项目类别: cleanText(row.querySelector(`td:nth-child(${4 + offset})`).textContent),
            开课地点,
            实施时间,
            选课截止时间: 截止状态,
            选课人数_容纳人数: 选课状态,
            授课教师: cleanText(row.querySelector(`td:nth-child(${15 + offset})`).textContent),
            isInvalid,
            locationMatch
        };
    }

    /**
     * 选课页课程监控与推送
     */
    function handleCoursePage() {
        console.log('[课程监控] 检测到选课页面，开始处理选课信息...');

        // 清除登录失败状态
        GM_setValue('loginFailStatus', false);
        GM_setValue('loginFailTime', 0);

        window.addEventListener('load', function() {
            const courseTable = document.getElementById('c_app_page_index_XuanKe_table');
            if (!courseTable) {
                console.error('[课程监控] 错误：未找到课程表格，请先登录系统');
                return;
            }
            const courseRows = courseTable.querySelectorAll('tbody .c--tr');
            if (courseRows.length === 0) {
                console.log('[课程监控] 提示：当前无课程数据或页面未加载完成');
                return;
            }

            // 提取并筛选课程
            let allCourses = Array.from(courseRows).map(row => extractCourseInfo(row));
            const validCourses = allCourses.filter(course => {
                return course.locationMatch && !course.isInvalid;
            });

            console.log('%c[课程监控] 符合推送条件的课程（未失效+地点匹配）', 'color:#2E86AB; font-size:16px; font-weight:bold;');
            console.log(`共 ${validCourses.length} 门：`, validCourses);

            // 本地存储处理
            const storedUniqueIds = GM_getValue('pushedCourseUniqueIds', []);
            console.log('%c[课程监控] 本地存储初始状态', 'color:#7B2CBF; font-weight:bold;');
            console.log('已存储的推送课程唯一标识：', storedUniqueIds);

            let pushedUniqueIds = new Set(storedUniqueIds);
            const currentValidUniqueIds = new Set(validCourses.map(course => course.uniqueId));

            // 筛选新课程
            const newCourses = validCourses.filter(course => !pushedUniqueIds.has(course.uniqueId));
            console.log('%c[课程监控] 新课程检测', 'color:#7B2CBF; font-weight:bold;');
            console.log('本次新增符合条件的课程：', newCourses.map(c => c.uniqueId));

            // 清理过期课程
            const allCurrentUniqueIds = new Set(allCourses.map(c => c.uniqueId));
            const currentInvalidUniqueIds = Array.from(allCurrentUniqueIds).filter(id => {
                const course = allCourses.find(c => c.uniqueId === id);
                return course?.isInvalid;
            });
            const currentLocationMismatchUniqueIds = Array.from(allCurrentUniqueIds).filter(id => {
                const course = allCourses.find(c => c.uniqueId === id);
                return !course?.locationMatch;
            });

            const expiredUniqueIds = Array.from(pushedUniqueIds).filter(id => {
                return !allCurrentUniqueIds.has(id)
                    || currentInvalidUniqueIds.includes(id)
                    || currentLocationMismatchUniqueIds.includes(id);
            });

            console.log('%c[课程监控] 过期课程清理', 'color:#7B2CBF; font-weight:bold;');
            console.log('待清理的课程唯一标识：', expiredUniqueIds);

            if (expiredUniqueIds.length > 0) {
                expiredUniqueIds.forEach(id => pushedUniqueIds.delete(id));
                const updatedAfterClean = Array.from(pushedUniqueIds);
                GM_setValue('pushedCourseUniqueIds', updatedAfterClean);
                console.log('清理后同步到本地的课程唯一标识：', updatedAfterClean);
            }

            // 格式化推送内容
            function formatToMarkdown(courses) {
                if (courses.length === 0) return '当前无新增有效课程';
                let md = '| 序号 | 项目名称 | 项目类别 | 实施时间 | 开课地点 | 选课情况 | 教师 |\n';
                md += '|------|----------|----------|----------|----------|----------|------|\n';
                courses.forEach(course => {
                    md += `| ${course.序号} | ${course.项目名称} | ${course.项目类别} | ${course.实施时间} | ${course.开课地点} | ${course.选课人数_容纳人数} | ${course.授课教师} |\n`;
                });
                return md + `\n提取时间：${new Date().toLocaleString()}`;
            }

            // 执行推送
            if (newCourses.length > 0) {
                console.log(`%c[课程监控] 发现 ${newCourses.length} 门符合条件的新课程，准备推送`, 'color:green;');
                pushToWechat(PUSH_TITLE, formatToMarkdown(newCourses));
                // 更新存储
                newCourses.forEach(course => pushedUniqueIds.add(course.uniqueId));
                GM_setValue('pushedCourseUniqueIds', Array.from(pushedUniqueIds));
            } else {
                console.log('[课程监控] 提示：无符合条件的新课程需要推送');
            }

            // 定时刷新
            setTimeout(() => {
                console.log(`\n[课程监控] ${REFRESH_INTERVAL/1000/60}分钟后自动刷新...`);
                window.location.reload();
            }, REFRESH_INTERVAL);
        });
    }
    // ==============================================================

    // ==================== 主流程分发 ====================
    if (currentUrl.includes('auth.seu.edu.cn/dist')) {
        handleLoginPage();
    } else if (/^https:\/\/labor\.seu\.edu\.cn\/System\/Home/.test(currentUrl)) {
        handleLaborHomePage();
    } else if (currentUrl.includes('labor.seu.edu.cn/SJItemKaiKe/XuanKe/Index')) {
        handleCoursePage();
    }
    // ====================================================
})();
