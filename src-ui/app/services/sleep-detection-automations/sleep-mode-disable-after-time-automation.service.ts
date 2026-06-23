import { Injectable } from '@angular/core';
import { AutomationConfigService } from '../automation-config.service';
import {
  AUTOMATION_CONFIGS_DEFAULT,
  SleepModeDisableAfterTimeAutomationConfig,
} from '../../models/automations';

import { distinctUntilChanged, map } from 'rxjs';
import { SleepService } from '../sleep.service';
import { time_to_ms } from 'src-ui/app/utils/time';
import { error, warn, info } from '@tauri-apps/plugin-log';

@Injectable({
  providedIn: 'root',
})
export class SleepModeDisableAfterTimeAutomationService {
  private config: SleepModeDisableAfterTimeAutomationConfig = structuredClone(
    AUTOMATION_CONFIGS_DEFAULT.SLEEP_MODE_DISABLE_AFTER_TIME
  );

  private timeout: NodeJS.Timeout | null = null;
  private ClearTimeout: NodeJS.Timeout | null = null;
  private SleepEnableTimeout: NodeJS.Timeout | null = null;
  constructor(
    private automationConfig: AutomationConfigService,
    private sleep: SleepService
  ) { }
  log(params: string) {
    info("SleepModeDisableAfterTimeAutomationService: " + params);

  }
  async init() {
    this.automationConfig.configs
      .pipe(map((configs) => configs.SLEEP_MODE_DISABLE_AFTER_TIME))
      .subscribe((config) => {
        this.config = config;
        if (!this.config.enabled) {
          this.log("config, clearing (1)");
          if (this.ClearTimeout) {
            this.log("config, clearing (2)");
            clearTimeout(this.ClearTimeout);
          }
          if (this.timeout) {
            this.log("config, clearing (3)");
            clearTimeout(this.timeout);
          }
        }
      });

    this.sleep.mode.pipe(distinctUntilChanged()).subscribe((mode) => {
      if (!this.config.enabled) {
        this.log("sleep mode !enabled");
        return;
      }
      if (!this.config.duration) {
        warn('SleepModeDisableAfterTimeAutomationService this.config.duration is null!');
        return;
      }
      if (mode) {
        if (this.SleepEnableTimeout) {
          this.log("clearTimeout(this.SleepEnableTimeout);");
          clearTimeout(this.SleepEnableTimeout);
        }
        this.SleepEnableTimeout = setTimeout(() => {
          this.log("SleepEnableTimeout");
          if (!this.config.duration) {
            error(
              'SleepModeDisableAfterTimeAutomationService this.config.duration is null! (2)'
            );
            this.log("SleepEnableTimeout exit");
            return;
          }
          if (this.ClearTimeout) {
            this.log("SleepEnableTimeout clear");
            clearTimeout(this.ClearTimeout);
          }
          if (!this.timeout) {
            this.timeout = setTimeout(() => this.disable(), time_to_ms(this.config.duration) - time_to_ms(this.config.sleep));
          }
        }, time_to_ms(this.config.sleep));
      } else {
        if (this.SleepEnableTimeout) {
          this.log("else (1)");
          clearTimeout(this.SleepEnableTimeout);
        }
        if (this.ClearTimeout) {
          this.log("else (2)");
          clearTimeout(this.ClearTimeout);
        }
        if (this.config.awake) {
          this.log("else (3)");
          this.ClearTimeout = setTimeout(() => {
            if (!this.timeout) {
              this.log("else (4)");
              warn('SleepModeDisableAfterTimeAutomationService upsie');
              return;
            }
            this.log("else (5)");
            clearTimeout(this.timeout);
          }, time_to_ms(this.config.awake));
        }
      }
    });
  }

  async disable() {
    if (!this.config.enabled) {
      this.log("disable canceled");
      return;
    }
    await this.sleep.disableSleepMode({
      type: 'AUTOMATION',
      automation: 'SLEEP_MODE_DISABLE_AFTER_TIME',
    });
  }
}
