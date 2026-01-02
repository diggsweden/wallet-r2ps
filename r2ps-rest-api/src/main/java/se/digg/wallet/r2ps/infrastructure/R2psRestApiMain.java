package se.digg.wallet.r2ps.infrastructure;

import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;
import org.springframework.scheduling.annotation.EnableScheduling;

@SpringBootApplication
@EnableScheduling
public class R2psRestApiMain {
  public static void main(String[] args) {
    SpringApplication.run(R2psRestApiMain.class, args);
  }
}
