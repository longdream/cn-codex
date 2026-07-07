#!/usr/bin/env python3
"""
股票获取评价 —— 输入某个股票代码，获取实时行情并输出综合评价。
支持 A 股、港股、美股主流代码（如 600519、00700、AAPL）。
依赖：pip install akshare pandas
"""

import sys
import json

try:
    import akshare as ak
    import pandas as pd
except ImportError as e:
    print(json.dumps({"error": f"缺少依赖: {e}。请执行: pip install akshare pandas"}, ensure_ascii=False))
    sys.exit(1)


def fetch_stock_quote(stock_code: str) -> dict:
    """获取股票实时行情并返回评价字典"""

    # 判断市场
    stock_code = stock_code.strip().upper()
    market = "A"
    if stock_code.startswith(("SH", "SZ", "BJ")):
        market = "A"
        symbol = stock_code
    elif stock_code.startswith(("HK", "0")) and len(stock_code) <= 6:
        market = "HK"
    elif stock_code.isalpha():
        market = "US"
    else:
        # 纯数字：默认 A 股，自动补前缀
        if stock_code.startswith(("6", "9")):
            symbol = f"SH{stock_code}"
        elif stock_code.startswith(("0", "3", "2")):
            symbol = f"SZ{stock_code}"
        elif stock_code.startswith(("4", "8")):
            symbol = f"BJ{stock_code}"
        else:
            symbol = f"SH{stock_code}"

    # --- A 股实时行情 ---
    try:
        df = ak.stock_zh_a_spot_em()
        df_match = df[df["代码"] == stock_code]
        if df_match.empty:
            # 尝试带前缀匹配
            df_match = df[df["代码"] == symbol.replace("SH","").replace("SZ","").replace("BJ","")]
        if df_match.empty:
            return {"error": f"未找到股票代码: {stock_code}"}

        row = df_match.iloc[0]
        name = row.get("名称", "")
        price = float(row.get("最新价", 0))
        change_pct = float(row.get("涨跌幅", 0))
        volume = float(row.get("成交量", 0))
        amount = float(row.get("成交额", 0))
        turnover = row.get("换手率", "N/A")
        pe = row.get("市盈率-动态", "N/A")
        pb = row.get("市净率", "N/A")
        total_mv = row.get("总市值", "N/A")
        amplitude = row.get("振幅", "N/A")
        high = row.get("最高", 0)
        low = row.get("最低", 0)
        open_px = row.get("今开", 0)

        # 综合评价逻辑
        signals = []
        scores = []
        if isinstance(pe, (int, float)) and pe > 0:
            if pe < 15:
                signals.append("市盈率较低，估值偏合理")
                scores.append(85)
            elif pe < 30:
                signals.append("市盈率适中")
                scores.append(65)
            elif pe < 60:
                signals.append("市盈率偏高，关注成长性支撑")
                scores.append(45)
            else:
                signals.append("市盈率极高，估值风险较大")
                scores.append(25)
        elif isinstance(pe, (int, float)) and pe < 0:
            signals.append("公司处于亏损状态，市盈率为负")
            scores.append(20)
        else:
            signals.append("市盈率数据缺失")
            scores.append(50)

        if isinstance(pb, (int, float)):
            if pb < 1:
                signals.append("市净率低于1，可能破净")
                scores.append(40)
            elif pb < 3:
                signals.append("市净率较低")
                scores.append(75)
            elif pb < 8:
                signals.append("市净率适中")
                scores.append(55)
            else:
                signals.append("市净率较高")
                scores.append(35)

        # 涨跌幅评分
        if abs(change_pct) < 2:
            signals.append("当日走势平稳")
            scores.append(60)
        elif change_pct > 5:
            signals.append("当日大幅上涨，注意追高风险")
            scores.append(45)
        elif change_pct < -5:
            signals.append("当日大幅下跌，关注是否有负面事件")
            scores.append(30)
        elif change_pct > 2:
            signals.append("当日温和上涨")
            scores.append(70)
        elif change_pct < -2:
            signals.append("当日温和下跌，建议观望")
            scores.append(40)

        avg_score = sum(scores) / len(scores) if scores else 50
        if avg_score >= 70:
            overall = "★★★☆☆ 估值合理，可关注"
        elif avg_score >= 50:
            overall = "★★☆☆☆ 中性偏谨慎，建议进一步研究"
        elif avg_score >= 35:
            overall = "★☆☆☆☆ 估值偏高或存在风险，需谨慎"
        else:
            overall = "☆☆☆☆☆ 风险较大，建议回避"

        return {
            "股票代码": stock_code,
            "股票名称": name,
            "最新价": price,
            "涨跌幅(%)": change_pct,
            "今开": open_px,
            "最高": high,
            "最低": low,
            "成交量(手)": volume,
            "成交额(元)": amount,
            "换手率(%)": turnover,
            "市盈率(动态)": pe,
            "市净率": pb,
            "总市值": total_mv,
            "振幅(%)": amplitude,
            "综合评价信号": signals,
            "综合评分(百分制)": round(avg_score, 1),
            "总体评价": overall,
        }
    except Exception as e:
        return {"error": f"数据获取失败: {e}"}


def main():
    if len(sys.argv) < 2:
        print("使用方法: python stock_evaluate.py <股票代码>")
        print("示例: python stock_evaluate.py 600519")
        sys.exit(1)

    stock_code = sys.argv[1]
    result = fetch_stock_quote(stock_code)
    print(json.dumps(result, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()