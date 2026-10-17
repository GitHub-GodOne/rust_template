import { Tooltip } from "antd";
import type { DashboardTrendPoint } from "../../api/admin/dashboard";
import { buildTrendBars } from "./homeOverview";

/** Seven-day stacked column chart: regular operations with errors on top. */
export function ActivityTrend({ points }: { points: DashboardTrendPoint[] }) {
  const { max, bars } = buildTrendBars(points);
  const ticks = Number.isInteger(max / 2) ? [max, max / 2, 0] : [max, 0];

  return (
    <figure className="home-trend" aria-label="近 7 日系统活动">
      <div className="home-trend-legend" aria-hidden="true">
        <span>
          <i className="home-trend-swatch is-normal" />
          正常操作
        </span>
        <span>
          <i className="home-trend-swatch is-error" />
          错误
        </span>
      </div>
      <div className="home-trend-plot">
        <div className="home-trend-axis" aria-hidden="true">
          {ticks.map((tick) => (
            <span key={tick}>{tick.toLocaleString()}</span>
          ))}
        </div>
        <div className="home-trend-area">
          <div className="home-trend-grid" aria-hidden="true">
            {ticks.map((tick) => (
              <i key={tick} />
            ))}
          </div>
          <div className="home-trend-columns">
            {bars.map((bar) => (
              <Tooltip
                key={bar.date}
                title={
                  <div className="home-trend-tooltip">
                    <strong>{bar.date}</strong>
                    <span>操作总数 {bar.total.toLocaleString()}</span>
                    <span>其中错误 {bar.errors.toLocaleString()}</span>
                  </div>
                }
              >
                <div
                  className={`home-trend-column${bar.isToday ? " is-today" : ""}`}
                >
                  <div className="home-trend-bar-slot">
                    {bar.isToday && (
                      <span
                        className="home-trend-value"
                        style={{ bottom: `${bar.totalPercent}%` }}
                      >
                        {bar.total.toLocaleString()}
                      </span>
                    )}
                    <div
                      className="home-trend-bar"
                      style={{ height: `${bar.totalPercent}%` }}
                    >
                      {bar.errors > 0 && (
                        <span
                          className="home-trend-segment is-error"
                          style={{
                            flexBasis: `${(bar.errors / Math.max(bar.total, 1)) * 100}%`,
                          }}
                        />
                      )}
                      {bar.normal > 0 && (
                        <span className="home-trend-segment is-normal" />
                      )}
                    </div>
                  </div>
                  <span className="home-trend-label">{bar.label}</span>
                </div>
              </Tooltip>
            ))}
          </div>
        </div>
      </div>
      <table className="home-sr-only">
        <caption>近 7 日系统活动</caption>
        <thead>
          <tr>
            <th scope="col">日期</th>
            <th scope="col">操作总数</th>
            <th scope="col">错误</th>
          </tr>
        </thead>
        <tbody>
          {bars.map((bar) => (
            <tr key={bar.date}>
              <th scope="row">{bar.date}</th>
              <td>{bar.total}</td>
              <td>{bar.errors}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </figure>
  );
}
